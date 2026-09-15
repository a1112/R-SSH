"""Assemble existing native packages and bind archive bytes to a verified binary."""

import hashlib
import json
import os
import platform
import shutil
import stat
import tarfile
import tomllib
import zipfile
from pathlib import Path, PurePosixPath


def digest_file(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_archive(archive, payload, expected_binary_hash, consumer_commit):
    expected = {}
    for parent, directories, files in os.walk(payload):
        for name in directories + files:
            info = (Path(parent) / name).lstat()
            if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
                raise ValueError("package payload contains a link or reparse point")
        for name in files:
            path = Path(parent) / name
            expected["payload/" + path.relative_to(payload).as_posix()] = digest_file(path)
    actual = {}
    with (zipfile.ZipFile(archive) if archive.suffix == ".zip" else tarfile.open(archive)) as bundle:
        entries = bundle.infolist() if archive.suffix == ".zip" else bundle.getmembers()
        for entry in entries:
            name = entry.filename if archive.suffix == ".zip" else entry.name
            if ("\\" in name or ":" in name or PurePosixPath(name).is_absolute()
                    or ".." in PurePosixPath(name).parts):
                raise ValueError("unsafe package archive member")
            if archive.suffix == ".zip":
                if stat.S_ISLNK(entry.external_attr >> 16):
                    raise ValueError("linked package archive member")
                if entry.is_dir():
                    continue
                stream = bundle.open(entry)
            else:
                if entry.isdir():
                    continue
                if not entry.isfile():
                    raise ValueError("non-regular package archive member")
                stream = bundle.extractfile(entry)
            if name in actual:
                stream.close()
                raise ValueError("duplicate package archive member")
            with stream:
                actual[name] = hashlib.file_digest(stream, "sha256").hexdigest()
    if actual != expected:
        raise ValueError("package archive differs from assembled payload")
    manifest = json.loads((payload / "manifest.json").read_text(encoding="utf-8"))
    if (not isinstance(manifest, dict)
            or any(not isinstance(manifest.get(key), dict) for key in ("artifact", "package", "signing"))
            or not isinstance(manifest["artifact"].get("binary"), str)):
        raise ValueError("malformed native package manifest")
    binary = manifest["artifact"]["binary"]
    if actual.get("payload/" + binary) != expected_binary_hash:
        raise ValueError("packaged executable differs from verified build")
    if manifest["package"]["source_commit"] != consumer_commit or manifest["signing"]["unsigned"] is not True:
        raise ValueError("package provenance or unsigned status mismatch")
    return binary


def assemble_package(consumer, target, artifact, receipt, output, mode, environment, run_command, verify_path):
    result = {"ok": False, "commands": []}
    try:
        host = {"Windows": "windows", "Linux": "linux", "Darwin": "macos"}[platform.system()]
        arch = {"AMD64": "x86_64", "x86_64": "x86_64", "ARM64": "aarch64", "arm64": "aarch64", "aarch64": "aarch64"}[platform.machine()]
        runtime = f"{host}-{arch}"
        pty = "windows-conpty" if host == "windows" else "unix-pty"
        extension = "zip" if host == "windows" else "tar.gz"
        name = f"rssh-{mode}-{runtime}-unsigned.{extension}"
        package_output = output / f"{mode}-package"
        verify_path(output, package_output)
        package_output.mkdir(parents=True, exist_ok=False)
        verify_path(output, package_output)
        payload = package_output / "payload"
        archive = package_output / name
        app = tomllib.loads((consumer / "crates/rssh-app/Cargo.toml").read_text())["package"]
        version = app["version"]
        if isinstance(version, dict):
            version = tomllib.loads((consumer / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        values = [str(target / artifact["path"]), str(payload), name, runtime, pty, version]
        if host == "windows":
            command = [shutil.which("pwsh") or "powershell.exe", "-NoProfile", "-NonInteractive", "-File", str(consumer / "scripts/ci/package-native.ps1")]
            flags = ["-Binary", "-PackageRoot", "-ArtifactName", "-RuntimeTarget", "-PtyBackend", "-Version"]
            unsigned = "-Unsigned"
        else:
            command = ["bash", str(consumer / "scripts/ci/package-native.sh")]
            flags = ["--binary", "--package-root", "--artifact-name", "--runtime-target", "--pty-backend", "--version"]
            unsigned = "--unsigned"
        for flag, value in zip(flags, values):
            command.extend([flag, value])
        command.append(unsigned)
        result["commands"].append(run_command(command, consumer, {**environment, "GITHUB_SHA": receipt["consumer_commit"]}, "native-package"))
        if result["commands"][-1]["returncode"] != 0:
            raise ValueError("native packaging command failed")
        verify_path(output, archive)
        verify_path(output, payload)
        binary = verify_archive(archive, payload, artifact["sha256"], receipt["consumer_commit"])
        result.update(ok=True, path=archive.relative_to(output).as_posix(),
                      sha256=digest_file(archive), size_bytes=archive.stat().st_size,
                      binary=binary, binary_sha256=artifact["sha256"],
                      runtime_target=runtime, profile=receipt["profile"],
                      consumer_commit=receipt["consumer_commit"], source_commit=receipt["source_commit"])
    except (OSError, ValueError, KeyError, zipfile.BadZipFile, tarfile.TarError) as error:
        result["error"] = str(error)
    return result

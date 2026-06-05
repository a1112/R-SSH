use std::{
    env,
    error::Error,
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::cli::DoctorOptions;
use rssh_pty::{PtyBackend, PtyCommand};

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

const REQUIRED_CONSOLE_TOOLS: &[&str] = &["ssh", "sftp", "scp"];

pub fn print_doctor(options: &DoctorOptions) -> Result<(), Box<dyn Error>> {
    let report = diagnose_console_dependencies();

    if options.json {
        println!("{}", doctor_json(&report)?);
    } else {
        for check in &report.checks {
            match &check.path {
                Some(path) => println!("ok\t{}\t{}", check.name, path),
                None => println!("missing\t{}", check.name),
            }
        }
    }

    if report.ok {
        return Ok(());
    }

    Err(doctor_error(format!(
        "doctor failed: missing {}",
        report
            .checks
            .iter()
            .filter(|check| !check.ok)
            .map(|check| check.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

pub fn doctor_json(report: &DoctorReport) -> Result<String, Box<dyn Error>> {
    Ok(serde_json::to_string(report)?)
}

pub fn diagnose_console_dependencies() -> DoctorReport {
    let paths = env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    let default_shell = PtyCommand::default_shell();

    diagnose_console_dependencies_in_paths_for_shell(&paths, default_shell.program())
}

pub fn diagnose_console_dependencies_in_paths_for_shell(
    paths: &[PathBuf],
    default_shell: &str,
) -> DoctorReport {
    let default_shell_path = resolve_command_path(default_shell, paths);
    let mut checks = vec![
        DoctorCheck {
            name: "pty-backend".to_owned(),
            ok: true,
            detail: Some(pty_backend_name(PtyBackend::current_platform()).to_owned()),
            path: None,
        },
        DoctorCheck {
            name: "default-shell".to_owned(),
            ok: default_shell_path.is_some(),
            detail: None,
            path: default_shell_path.map(|path| path.display().to_string()),
        },
    ];
    checks.extend(REQUIRED_CONSOLE_TOOLS.iter().map(|tool| {
        let path = find_command_in_paths(tool, paths);
        DoctorCheck {
            name: (*tool).to_owned(),
            ok: path.is_some(),
            detail: None,
            path: path.map(|path| path.display().to_string()),
        }
    }));

    DoctorReport {
        ok: checks.iter().all(|check| check.ok),
        checks,
    }
}

fn pty_backend_name(backend: PtyBackend) -> &'static str {
    match backend {
        PtyBackend::WindowsConpty => "windows-conpty",
        PtyBackend::UnixPty => "unix-pty",
    }
}

fn resolve_command_path(command: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    let command_path = PathBuf::from(command);
    if command_path.is_absolute() || command_path.components().count() > 1 {
        return command_path.is_file().then_some(command_path);
    }

    find_command_in_paths(command, paths)
}

fn find_command_in_paths(command: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    for path in paths {
        for candidate in command_candidates(command, path) {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

fn command_candidates(command: &str, directory: &Path) -> Vec<PathBuf> {
    executable_extensions()
        .into_iter()
        .map(|extension| directory.join(format!("{command}{extension}")))
        .collect()
}

fn executable_extensions() -> Vec<String> {
    if !cfg!(windows) {
        return vec![String::new()];
    }

    env::var_os("PATHEXT")
        .and_then(|value| value.into_string().ok())
        .map(|value| {
            value
                .split(';')
                .filter(|extension| !extension.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|extensions| !extensions.is_empty())
        .unwrap_or_else(|| {
            [".COM", ".EXE", ".BAT", ".CMD"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
}

fn doctor_error(message: String) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::NotFound, message))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    fn temp_tool_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("rssh-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn tool_file_name(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_owned()
        }
    }

    #[test]
    fn diagnoses_required_console_tools_from_search_paths() {
        let dir = temp_tool_dir("doctor-tools");
        for tool in ["shell", "ssh", "sftp", "scp"] {
            fs::write(dir.join(tool_file_name(tool)), "").unwrap();
        }

        let report = super::diagnose_console_dependencies_in_paths_for_shell(
            std::slice::from_ref(&dir),
            "shell",
        );
        let _ = fs::remove_dir_all(&dir);

        assert!(report.ok);
        assert_eq!(report.checks.len(), 5);
        assert!(report.checks.iter().all(|check| check.ok));
        assert!(matches!(
            report
                .checks
                .first()
                .and_then(|check| check.detail.as_deref()),
            Some("windows-conpty" | "unix-pty")
        ));
        assert_eq!(
            report
                .checks
                .iter()
                .map(|check| check.name.as_str())
                .collect::<Vec<_>>(),
            vec!["pty-backend", "default-shell", "ssh", "sftp", "scp"]
        );
    }

    #[test]
    fn reports_missing_console_tools_as_json() {
        let report = super::DoctorReport {
            ok: false,
            checks: vec![super::DoctorCheck {
                name: "ssh".to_owned(),
                ok: false,
                detail: None,
                path: None,
            }],
        };

        assert_eq!(
            super::doctor_json(&report).unwrap(),
            "{\"ok\":false,\"checks\":[{\"name\":\"ssh\",\"ok\":false}]}"
        );
    }
}

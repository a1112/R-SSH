# R-SSH gpu-allocator 0.28 patch

This directory preserves gpu-allocator 0.28.0 under its upstream MIT and
Apache-2.0 licenses.

R-SSH pins its permissive Windows dependency range to 0.62.2. wgpu-hal 30
uses the Windows 0.62 ABI directly; allowing Cargo to resolve gpu-allocator to
Windows 0.61 produces incompatible Direct3D 12 types.

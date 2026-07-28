# R-SSH glyphon 0.12 patch

This directory preserves glyphon 0.12.0 under its upstream MIT, Apache-2.0,
and Zlib licenses.

R-SSH carries two narrow integration changes:

- disable cosmic-text default features and enable only `std` and `swash`, so
  the renderer cannot discover system fonts through glyphon's dependency;
- expose physical mask/color atlas dimensions and enforce a combined texture
  byte cap before atlas growth.

The Windows dependency pin keeps `gpu-allocator` and `wgpu-hal` on the same
Windows crate ABI selected by wgpu 30.

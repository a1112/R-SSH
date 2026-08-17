    #[test]
    fn window_app_dispatches_resize_for_active_pane() {
        let resizes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&resizes);
        let mut app = NativeWindowApp::new_with_command(None, rssh_pty::PtyCommand::new("pwsh"));
        app.resize_handler = Box::new(move |resize| {
            recorded.lock().unwrap().push(*resize);
            true
        });
        let active_pane = app.app_shell.active_pane_id();

        app.handle_window_resize(PhysicalSize::new(96, 80)).unwrap();

        assert_eq!(
            resizes.lock().unwrap().as_slice(),
            [NativeWindowResize {
                window_id: rssh_core::WindowId::new(1),
                pane: active_pane,
                pixel_width: 96,
                pixel_height: 80,
                terminal_size: rssh_core::TerminalSize::new(10, 3),
                is_full_screen: false,
            }]
        );
        assert_eq!(
            app.runtime.terminal().grid().size(),
            rssh_core::TerminalSize::new(10, 3)
        );
    }

    #[test]
    fn window_app_resize_event_reports_fullscreen_state() {
        let resizes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&resizes);
        let mut app = NativeWindowApp::new(None);
        app.resize_handler = Box::new(move |resize| {
            recorded.lock().unwrap().push(*resize);
            true
        });

        app.command_palette_execute(WindowCommand::ToggleFullScreen);
        resizes.lock().unwrap().clear();
        app.handle_window_resize(PhysicalSize::new(160, 96))
            .unwrap();

        assert_eq!(
            resizes.lock().unwrap().as_slice(),
            [NativeWindowResize {
                window_id: rssh_core::WindowId::new(1),
                pane: rssh_core::PaneId::new(1),
                pixel_width: 160,
                pixel_height: 96,
                terminal_size: rssh_core::TerminalSize::new(17, 4),
                is_full_screen: true,
            }]
        );
    }

    #[test]
    fn window_app_parses_static_wezterm_resized_status_setter() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r#"
            local wezterm = require 'wezterm'

            wezterm.on('window-resized', function(window, pane)
              local dims = window:get_dimensions()
              window:set_right_status(dims.pixel_width .. 'x' .. dims.pixel_height)
            end)
            "#,
        )
        .expect("expected static WezTerm resized event status setter");
        app.set_config_overrides(overrides);

        app.handle_window_resize(PhysicalSize::new(160, 96))
            .unwrap();

        assert_eq!(app.right_status, "160x96");
    }

    #[test]
    fn encodes_window_mouse_events_as_sgr_sequences_when_enabled() {
        let mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Sgr);

        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[<0;3;2M"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Up(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[<0;3;2m"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::ScrollDown,
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[<65;3;2M"
        );
    }

    #[test]
    fn encodes_window_mouse_events_as_sgr_pixels_sequences_when_enabled() {
        let mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::SgrPixels);

        assert_eq!(
            encode_window_mouse_event_with_pixels(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                21,
                34,
                mode,
            )
            .unwrap(),
            b"\x1b[<0;21;34M"
        );
        assert_eq!(
            encode_window_mouse_event_with_pixels(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Up(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                21,
                34,
                mode,
            )
            .unwrap(),
            b"\x1b[<0;21;34m"
        );
    }

    #[test]
    fn encodes_window_mouse_events_as_legacy_sequences_when_sgr_is_disabled() {
        let mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::X10);

        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 0,
                    row: 0,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[M !!"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Up(MouseButton::Left),
                    column: 0,
                    row: 0,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[M#!!"
        );
    }

    #[test]
    fn encodes_window_mouse_events_as_utf8_sequences_when_enabled() {
        let mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Utf8);

        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 95,
                    row: 96,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[M \xc2\x80\xc2\x81"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Up(MouseButton::Left),
                    column: 95,
                    row: 96,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[M#\xc2\x80\xc2\x81"
        );
    }

    #[test]
    fn encodes_window_mouse_events_as_urxvt_sequences_when_enabled() {
        let mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Urxvt);

        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 0,
                    row: 0,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[32;1;1M"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Up(MouseButton::Left),
                    column: 0,
                    row: 0,
                    modifiers: ModifiersState::empty(),
                },
                mode,
            )
            .unwrap(),
            b"\x1b[35;1;1M"
        );
    }

    #[test]
    fn encodes_window_mouse_drag_and_motion_events_when_enabled() {
        let button_event_mode =
            MouseInputMode::new(MouseReportingMode::ButtonEvent, MouseProtocolMode::Sgr);
        let any_event_mode =
            MouseInputMode::new(MouseReportingMode::AnyEvent, MouseProtocolMode::Sgr);

        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Drag(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                button_event_mode,
            )
            .unwrap(),
            b"\x1b[<32;3;2M"
        );
        assert_eq!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Moved,
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                any_event_mode,
            )
            .unwrap(),
            b"\x1b[<35;3;2M"
        );
    }

    #[test]
    fn ignores_window_mouse_motion_events_outside_matching_reporting_modes() {
        let normal_mode = MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Sgr);
        let button_event_mode =
            MouseInputMode::new(MouseReportingMode::ButtonEvent, MouseProtocolMode::Sgr);

        assert!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Drag(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                normal_mode,
            )
            .is_none()
        );
        assert!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Moved,
                    column: 2,
                    row: 1,
                    modifiers: ModifiersState::empty(),
                },
                button_event_mode,
            )
            .is_none()
        );
    }

    #[test]
    fn ignores_window_mouse_events_when_reporting_is_disabled() {
        assert!(
            encode_window_mouse_event(
                WindowMouseEvent {
                    kind: WindowMouseEventKind::Down(MouseButton::Left),
                    column: 0,
                    row: 0,
                    modifiers: ModifiersState::empty(),
                },
                MouseInputMode::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn encodes_window_application_keypad_keys_when_enabled() {
        assert_eq!(
            encode_window_key(
                &Key::Character("1".into()),
                PhysicalKey::Code(WinitKeyCode::Numpad1),
                Some("1"),
                ModifiersState::empty(),
                false,
                true
            ),
            b"\x1bOq"
        );
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::Enter),
                PhysicalKey::Code(WinitKeyCode::NumpadEnter),
                Some("\r"),
                ModifiersState::empty(),
                false,
                true
            ),
            b"\x1bOM"
        );
        assert!(
            encode_window_key(
                &Key::Character("1".into()),
                PhysicalKey::Code(WinitKeyCode::Numpad1),
                Some("1"),
                ModifiersState::CONTROL,
                false,
                true
            )
            .is_empty()
        );
    }

    #[test]
    fn encodes_window_backtab_and_function_keys_for_pty() {
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::Tab),
                PhysicalKey::Code(WinitKeyCode::Tab),
                Some("\t"),
                ModifiersState::SHIFT,
                false,
                false
            ),
            b"\x1b[Z"
        );
        assert_eq!(
            encode_window_key(
                &Key::Named(NamedKey::F12),
                PhysicalKey::Code(WinitKeyCode::F12),
                None,
                ModifiersState::empty(),
                false,
                false
            ),
            b"\x1b[24~"
        );
    }

    #[test]
    fn window_app_rebuilds_snapshot_from_pty_output() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"live").unwrap();

        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('l'));
        assert_eq!(snapshot_char(&app.snapshot, 0, 3), Some('e'));
    }

    #[test]
    fn window_app_updates_live_snapshot_from_terminal_damage() {
        let mut app = NativeWindowApp::new(None);

        app.handle_pty_output(b"live").unwrap();

        let metrics = app.metrics_snapshot();
        assert_eq!(snapshot_char(&app.snapshot, 0, 0), Some('l'));
        assert_eq!(snapshot_char(&app.snapshot, 0, 3), Some('e'));
        assert_eq!(metrics.snapshot_damage_updates, 1);
        assert_eq!(metrics.snapshot_rebuilds, 0);
    }

    #[test]
    fn window_app_renders_pending_terminal_damage_to_framebuffer() {
        let mut app = NativeWindowApp::new(None);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        app.handle_pty_output(b"live").unwrap();

        assert_eq!(
            app.pending_frame_damage,
            vec![DamageRegion::new(0, 0, 4, 1)]
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Damage);
        assert!(app.pending_frame_damage.is_empty());

        let metrics = app.metrics_snapshot();
        assert_eq!(metrics.full_render_frames, 1);
        assert_eq!(metrics.dirty_render_frames, 1);
    }

    #[test]
    fn native_gpu_damage_is_consumed_only_after_successful_present() {
        let damage = DamageRegion::new(1, 2, 3, 4);
        for outcome in [Ok(GpuFrameStatus::Skipped), Err("device fault")] {
            let expected_error = outcome.is_err();
            let mut pending = vec![damage];
            let mut needs_full = false;
            let result = finalize_native_gpu_frame(outcome, &mut pending, &mut needs_full);

            assert_eq!(pending, vec![damage]);
            assert!(needs_full);
            assert_eq!(result.is_err(), expected_error);
        }

        let mut pending = vec![damage];
        let mut needs_full = true;
        assert_eq!(
            finalize_native_gpu_frame(
                Ok::<_, &str>(GpuFrameStatus::Presented),
                &mut pending,
                &mut needs_full,
            ),
            Ok(true)
        );
        assert!(pending.is_empty());
        assert!(!needs_full);
    }

    #[test]
    fn deferred_gpu_ready_remains_initializing_until_a_presented_frame() {
        assert_eq!(
            deferred_gpu_initialization_owner(true),
            PresentationOwner::GpuInitializing
        );
        assert_eq!(
            presentation_owner_after_gpu_frame(
                PresentationOwner::GpuInitializing,
                GpuFrameStatus::Skipped,
            ),
            PresentationOwner::GpuInitializing
        );
        assert_eq!(
            presentation_owner_after_gpu_frame(
                PresentationOwner::GpuInitializing,
                GpuFrameStatus::Presented,
            ),
            PresentationOwner::GpuActive
        );
    }

    #[test]
    fn gpu_activation_releases_bootstrap_staging_and_cpu_fallback_can_reallocate() {
        let mut bootstrap_frame = Vec::with_capacity(4_096);
        bootstrap_frame.resize(4_096, 0xaa);

        super::release_bootstrap_staging_after_gpu_activation(&mut bootstrap_frame);

        assert!(bootstrap_frame.is_empty());
        assert_eq!(bootstrap_frame.capacity(), 0);

        bootstrap_frame.resize(256, 0);
        assert_eq!(bootstrap_frame.len(), 256);
        assert!(bootstrap_frame.capacity() >= 256);
    }

    #[test]
    fn deferred_gpu_initialization_failure_selects_cpu_fallback() {
        assert_eq!(
            deferred_gpu_initialization_owner(false),
            PresentationOwner::CpuFallback
        );
    }

    #[test]
    fn deferred_gpu_candidate_resizes_to_the_latest_window_size_before_install() {
        let latest_size = PhysicalSize::new(1_280, 720);
        let mut observed_size = None;

        let owner = super::resize_deferred_gpu_candidate_for_install(latest_size, |size| {
            observed_size = Some(size);
            Ok::<_, ()>(())
        })
        .unwrap();

        assert_eq!(observed_size, Some(latest_size));
        assert_eq!(owner, PresentationOwner::GpuInitializing);
    }

    #[test]
    fn deferred_gpu_candidate_resize_failure_selects_cpu_fallback() {
        let result = super::resize_deferred_gpu_candidate_for_install(
            PhysicalSize::new(1_280, 720),
            |_| Err("resize failed"),
        );

        assert_eq!(result, Err("resize failed"));
        assert_eq!(
            deferred_gpu_initialization_owner(result.is_ok()),
            PresentationOwner::CpuFallback
        );
    }

    #[test]
    fn deferred_gpu_worker_runs_off_the_event_loop_thread() {
        let event_loop_thread = thread::current().id();
        let (worker_thread_sender, worker_thread_receiver) = mpsc::channel();

        super::spawn_deferred_gpu_task("rssh-test-gpu-init".to_owned(), move || {
            worker_thread_sender.send(thread::current().id()).unwrap();
        })
        .unwrap();

        assert_ne!(
            worker_thread_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            event_loop_thread
        );
    }

    #[test]
    fn deferred_gpu_completion_ignores_stale_results_and_falls_back_on_current_failure() {
        let mut app = NativeWindowApp::new(None);
        app.set_renderer_mode(super::RendererMode::Auto);
        app.presentation_owner = PresentationOwner::GpuInitializing;
        app.deferred_gpu_generation = 7;
        app.frame_needs_full_repaint = false;
        app.metrics
            .startup_trace
            .mark_renderer(super::RendererKind::Gpu);

        assert!(!app.handle_deferred_gpu_initialized(
            6,
            super::DeferredGpuInitialization::Failed("stale".to_owned()),
        ));
        assert_eq!(
            app.presentation_owner,
            PresentationOwner::GpuInitializing
        );
        assert!(!app.frame_needs_full_repaint);

        assert!(app.handle_deferred_gpu_initialized(
            7,
            super::DeferredGpuInitialization::Failed("current".to_owned()),
        ));
        assert_eq!(app.presentation_owner, PresentationOwner::CpuFallback);
        assert!(app.frame_needs_full_repaint);
        assert_eq!(
            app.metrics.startup_trace.snapshot().final_renderer,
            Some(super::RendererKind::Cpu)
        );
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_background_color_for_framebuffer() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.colors = {
              background = '#010203',
            }

            return config
            "##,
        )
        .expect("expected WezTerm colors.background config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let terminal_origin_y = usize::from(TAB_BAR_ROWS) * CELL_HEIGHT as usize;
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                terminal_origin_y
            ),
            [1, 2, 3, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_background_color_below_gradient_layer() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.background = {
              {
                source = { Color = '#000000' },
              },
              {
                source = {
                  Gradient = {
                    orientation = 'Horizontal',
                    colors = { '#ffffff', '#ffffff' },
                    noise = 0,
                  },
                },
                opacity = 0.5,
              },
            }

            return config
            "##,
        )
        .expect("expected WezTerm background Color and Gradient layers");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().background_color,
            Color::Rgb(0, 0, 0)
        );
        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(127, 127, 127), Color::Rgb(127, 127, 127)],
            })
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let terminal_origin_y = usize::from(TAB_BAR_ROWS) * CELL_HEIGHT as usize;
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                terminal_origin_y
            ),
            [127, 127, 127, 255]
        );
    }

    #[test]
    fn window_app_prepends_window_background_gradient_before_background_layers() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.window_background_gradient = {
              orientation = 'Horizontal',
              colors = { '#ff0000', '#ff0000' },
              noise = 0,
            }

            config.background = {
              {
                source = { Color = '#0000ff' },
                opacity = 0.5,
              },
            }

            return config
            "##,
        )
        .expect("expected WezTerm background layers with prepended gradient");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().background,
            vec![
                super::NativeWindowBackgroundVisualLayer::Gradient(
                    NativeWindowBackgroundGradient {
                        orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                        interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                        blend: NativeWindowBackgroundGradientBlend::Rgb,
                        noise: Some(0),
                        segment: None,
                        preset: None,
                        opacity_alpha: u8::MAX,
                        blend_with_background_color: false,
                        hsb: super::native_identity_hsb(),
                        colors: vec![Color::Rgb(255, 0, 0), Color::Rgb(255, 0, 0)],
                    }
                ),
                super::NativeWindowBackgroundVisualLayer::Color(Color::Rgba(0, 0, 255, 127)),
            ]
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let terminal_origin_y = usize::from(TAB_BAR_ROWS) * CELL_HEIGHT as usize;
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                terminal_origin_y
            ),
            [128, 0, 127, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_background_color_below_preset_gradient_layer() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.background = {
              {
                source = { Color = '#000000' },
              },
              {
                source = {
                  Gradient = {
                    preset = 'Blues',
                    noise = 0,
                  },
                },
                opacity = 0.5,
              },
            }

            return config
            "##,
        )
        .expect("expected WezTerm background Color and preset Gradient layers");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: Some(NativeWindowBackgroundGradientPreset::Blues),
                opacity_alpha: 127,
                blend_with_background_color: true,
                hsb: super::native_identity_hsb(),
                colors: Vec::new(),
            })
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let width = FRAME_WIDTH as usize;
        assert_eq!(
            frame_pixel_at(&frame, width, 0, FRAME_HEIGHT as usize - 1),
            [123, 125, 127, 255]
        );
        assert_eq!(
            frame_pixel_at(
                &frame,
                width,
                FRAME_WIDTH as usize - 1,
                FRAME_HEIGHT as usize - 1
            ),
            [3, 23, 53, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_background_file_layer() {
        let image_path = write_test_png_file("wezterm-background-file-layer.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background File layer");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_bmp_file_layer() {
        let image_path = write_test_bmp_file("wezterm-background-file-layer.bmp");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm BMP background File layer");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_ico_file_layer() {
        let image_path = write_test_ico_file("wezterm-background-file-layer.ico");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm ICO background File layer");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_tiff_file_layer() {
        let image_path = write_test_tiff_file("wezterm-background-file-layer.tiff");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm TIFF background File layer");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_dds_file_layer() {
        let image_path = write_test_dds_file("wezterm-background-file-layer.dds");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm DDS background File layer");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_pnm_file_layer() {
        let image_path = write_test_ppm_file("wezterm-background-file-layer.ppm");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm PNM background File layer");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_tga_file_layer() {
        let image_path = write_test_tga_file("wezterm-background-file-layer.tga");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm TGA background File layer");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_farbfeld_file_layer() {
        let image_path = write_test_farbfeld_file("wezterm-background-file-layer.ff");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm farbfeld background File layer");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_file_layer_table_path() {
        let image_path = write_test_png_file("wezterm-background-file-layer-table-path.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = {{ path = '{lua_path}', speed = 0.2 }} }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background File table path layer");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_parses_wezterm_background_file_layer_attachment_scroll() {
        let image_path = write_test_png_file("wezterm-background-file-layer-attachment-scroll.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
                attachment = 'Scroll',
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background File layer attachment");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_images[0].attachment,
            RenderBackgroundImageAttachment::Scroll
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_parses_wezterm_background_file_layer_attachment_parallax() {
        let image_path =
            write_test_png_file("wezterm-background-file-layer-attachment-parallax.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
                attachment = {{ Parallax = 0.5 }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background File layer parallax attachment");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_images[0].attachment,
            RenderBackgroundImageAttachment::Parallax { factor_millis: 500 }
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_color_below_file_layer() {
        let image_path = write_test_png_file("wezterm-background-color-file-layer.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ Color = '#000000' }},
              }},
              {{
                source = {{ File = '{lua_path}' }},
                opacity = 0.5,
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background Color and File layers");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [127, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_stacked_file_layers() {
        let image_path = write_test_png_file("wezterm-background-stacked-file-layers.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ Color = '#000000' }},
              }},
              {{
                source = {{ File = '{lua_path}' }},
                opacity = 0.5,
              }},
              {{
                source = {{ File = '{lua_path}' }},
                opacity = 0.5,
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm stacked File background layers");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [190, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_gradient_over_file_layer() {
        let image_path = write_test_png_file("wezterm-background-gradient-over-file.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
              {{
                source = {{
                  Gradient = {{
                    orientation = 'Horizontal',
                    colors = {{ '#ffffff', '#ffffff' }},
                    noise = 0,
                  }},
                }},
                opacity = 0.5,
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm Gradient over File background layers");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 127, 127, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_color_over_file_layer() {
        let image_path = write_test_png_file("wezterm-background-file-color-layer.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
              }},
              {{
                source = {{ Color = '#0000ff' }},
                opacity = 0.5,
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background File and Color layers");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [128, 0, 127, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_file_layer_hsb() {
        let image_path = write_test_png_file("wezterm-background-file-layer-hsb.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
                hsb = {{ brightness = 0.5 }},
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background File layer hsb");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [128, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_legacy_window_background_image_layer() {
        let image_path = write_test_png_file("wezterm-window-background-image.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.window_background_image = '{lua_path}'
            config.window_background_image_hsb = {{ brightness = 0.5 }}
            config.window_background_opacity = 0.5

            return config
            "##
        ))
        .expect("expected legacy WezTerm window background image config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [69, 6, 6, 255]
        );
        let effective = app.native_effective_config();
        assert_eq!(
            effective.window_background_image,
            Some(image_path.to_string_lossy().to_string())
        );
        assert_eq!(
            effective.window_background_image_hsb,
            Some(NativeInactivePaneHsb {
                hue: NativeHsbMultiplier::from_f32(1.0),
                saturation: NativeHsbMultiplier::from_f32(1.0),
                brightness: NativeHsbMultiplier::from_f32(0.5),
            })
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_layers_wezterm_background_color_over_legacy_window_background_image() {
        let image_path = write_test_png_file("wezterm-window-background-image-under-color.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.window_background_image = '{lua_path}'
            config.background = {{
              {{
                source = {{ Color = '#0000ff' }},
                opacity = 0.5,
              }},
            }}

            return config
            "##
        ))
        .expect("expected legacy WezTerm window background image under Color layer config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().background,
            vec![
                super::NativeWindowBackgroundVisualLayer::Image(
                    super::NativeWindowBackgroundImage {
                        data: std::fs::read(&image_path).expect("expected test PNG data"),
                        opacity_alpha: u8::MAX,
                        hsb: super::native_identity_hsb(),
                        animation_speed_millis: 1_000,
                        attachment: RenderBackgroundImageAttachment::Fixed,
                        layout: super::NativeWindowBackgroundImageLayout {
                            width: super::RenderBackgroundImageDimension::Percent(10_000),
                            height: super::RenderBackgroundImageDimension::Percent(10_000),
                            ..super::NativeWindowBackgroundImageLayout::default()
                        },
                    },
                ),
                super::NativeWindowBackgroundVisualLayer::Color(Color::Rgba(0, 0, 255, 127)),
            ]
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [128, 0, 127, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_file_layer_fixed_layout() {
        let image_path = write_test_png_file("wezterm-background-file-layer-layout.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
                width = 1,
                height = 1,
                repeat_x = 'NoRepeat',
                repeat_y = 'NoRepeat',
                horizontal_align = 'Right',
                vertical_align = 'Bottom',
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background File layer fixed layout");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        assert_eq!(
            frame_pixel_at(&frame, FRAME_WIDTH as usize, 0, FRAME_HEIGHT as usize - 1),
            [12, 12, 12, 255]
        );
        assert_eq!(
            frame_pixel_at(
                &frame,
                FRAME_WIDTH as usize,
                FRAME_WIDTH as usize - 1,
                FRAME_HEIGHT as usize - 1
            ),
            [255, 0, 0, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_file_layer_percent_width() {
        let image_path = write_test_png_file("wezterm-background-file-layer-percent.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
                width = '50%',
                height = 1,
                repeat_x = 'NoRepeat',
                repeat_y = 'NoRepeat',
                horizontal_align = 'Left',
                vertical_align = 'Bottom',
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background File layer percentage width");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        let width = FRAME_WIDTH as usize;
        let bottom_y = FRAME_HEIGHT as usize - 1;
        assert_eq!(frame_pixel_at(&frame, width, 0, bottom_y), [255, 0, 0, 255]);
        assert_eq!(
            frame_pixel_at(&frame, width, FRAME_WIDTH as usize / 2 - 1, bottom_y),
            [255, 0, 0, 255]
        );
        assert_eq!(
            frame_pixel_at(&frame, width, FRAME_WIDTH as usize / 2, bottom_y),
            [12, 12, 12, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_background_file_layer_cell_repeat_size() {
        let image_path = write_test_png_file("wezterm-background-file-layer-cell-repeat.png");
        let lua_path = lua_string_path(&image_path);
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local config = {{}}

            config.background = {{
              {{
                source = {{ File = '{lua_path}' }},
                width = 1,
                height = '1cell',
                repeat_x = 'Repeat',
                repeat_y = 'NoRepeat',
                repeat_x_size = '2cell',
                horizontal_offset = '1cell',
                vertical_align = 'Bottom',
              }},
            }}

            return config
            "##
        ))
        .expect("expected WezTerm background File layer cell repeat size");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);
        let width = FRAME_WIDTH as usize;
        let bottom_y = FRAME_HEIGHT as usize - 1;
        let first_tile_x = CELL_WIDTH as usize;
        let second_tile_x = first_tile_x + CELL_WIDTH as usize * 2;
        assert_eq!(
            frame_pixel_at(&frame, width, first_tile_x - 1, bottom_y),
            [12, 12, 12, 255]
        );
        assert_eq!(
            frame_pixel_at(&frame, width, first_tile_x, bottom_y),
            [255, 0, 0, 255]
        );
        assert_eq!(
            frame_pixel_at(&frame, width, first_tile_x + 1, bottom_y),
            [12, 12, 12, 255]
        );
        assert_eq!(
            frame_pixel_at(&frame, width, second_tile_x, bottom_y),
            [255, 0, 0, 255]
        );
        assert_eq!(
            frame_pixel_at(
                &frame,
                width,
                first_tile_x,
                FRAME_HEIGHT as usize - CELL_HEIGHT as usize - 1
            ),
            [12, 12, 12, 255]
        );

        let _ = std::fs::remove_file(image_path);
    }

    #[test]
    fn window_app_renders_wezterm_vertical_window_background_gradient() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.window_background_gradient = {
              orientation = 'Vertical',
              colors = { '#010203', '#111213' },
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm window_background_gradient config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Vertical,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(1, 2, 3), Color::Rgb(17, 18, 19)],
            })
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let width = FRAME_WIDTH as usize;
        let terminal_origin_y = usize::from(TAB_BAR_ROWS) * CELL_HEIGHT as usize;
        assert_eq!(
            frame_pixel_at(&frame, width, CELL_WIDTH as usize, terminal_origin_y),
            [16, 17, 18, 255]
        );
        assert_eq!(
            frame_pixel_at(
                &frame,
                width,
                CELL_WIDTH as usize,
                FRAME_HEIGHT as usize - 1
            ),
            [1, 2, 3, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_linear_angle_window_background_gradient() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.window_background_gradient = {
              orientation = { Linear = { angle = 180.0 } },
              colors = { '#010203', '#111213' },
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm linear window_background_gradient config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Linear {
                    angle_millidegrees: 180_000,
                },
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(1, 2, 3), Color::Rgb(17, 18, 19)],
            })
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let width = FRAME_WIDTH as usize;
        assert_eq!(
            frame_pixel_at(&frame, width, 0, FRAME_HEIGHT as usize - 1),
            [17, 18, 19, 255]
        );
        assert_eq!(
            frame_pixel_at(
                &frame,
                width,
                FRAME_WIDTH as usize - 1,
                FRAME_HEIGHT as usize - 1
            ),
            [1, 2, 3, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_radial_window_background_gradient() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.window_background_gradient = {
              orientation = {
                Radial = {
                  cx = 0.0,
                  cy = 1.0,
                  radius = 1.0,
                },
              },
              colors = { '#010203', '#111213' },
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm radial window_background_gradient config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Radial {
                    cx_millis: 0,
                    cy_millis: 1_000,
                    radius_millis: 1_000,
                },
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(1, 2, 3), Color::Rgb(17, 18, 19)],
            })
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let width = FRAME_WIDTH as usize;
        assert_eq!(
            frame_pixel_at(&frame, width, 0, FRAME_HEIGHT as usize - 1),
            [1, 2, 3, 255]
        );
        assert_eq!(
            frame_pixel_at(
                &frame,
                width,
                FRAME_WIDTH as usize - 1,
                FRAME_HEIGHT as usize - 1
            ),
            [17, 18, 19, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_preset_window_background_gradient() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.window_background_gradient = {
              preset = 'Blues',
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm preset window_background_gradient config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: Some(NativeWindowBackgroundGradientPreset::Blues),
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: Vec::new(),
            })
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let width = FRAME_WIDTH as usize;
        assert_eq!(
            frame_pixel_at(&frame, width, 0, FRAME_HEIGHT as usize - 1),
            [247, 251, 255, 255]
        );
        assert_eq!(
            frame_pixel_at(
                &frame,
                width,
                FRAME_WIDTH as usize - 1,
                FRAME_HEIGHT as usize - 1
            ),
            [8, 48, 107, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_background_preset_gradient_layer_opacity() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.background = {
              {
                source = {
                  Gradient = {
                    preset = 'Blues',
                    noise = 0,
                  },
                },
                opacity = 0.5,
              },
            }

            return config
            "##,
        )
        .expect("expected WezTerm background preset gradient layer opacity config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let width = FRAME_WIDTH as usize;
        assert_eq!(
            frame_pixel_at(&frame, width, 0, FRAME_HEIGHT as usize - 1),
            [247, 251, 255, 127]
        );
        assert_eq!(
            frame_pixel_at(
                &frame,
                width,
                FRAME_WIDTH as usize - 1,
                FRAME_HEIGHT as usize - 1
            ),
            [8, 48, 107, 127]
        );
    }

    #[test]
    fn window_app_renders_wezterm_background_preset_gradient_layer_hsb() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.background = {
              {
                source = {
                  Gradient = {
                    preset = 'Blues',
                    noise = 0,
                  },
                },
                hsb = { brightness = 0.5 },
              },
            }

            return config
            "##,
        )
        .expect("expected WezTerm background preset gradient layer hsb config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let width = FRAME_WIDTH as usize;
        assert_eq!(
            frame_pixel_at(&frame, width, 0, FRAME_HEIGHT as usize - 1),
            [124, 126, 128, 255]
        );
        assert_eq!(
            frame_pixel_at(
                &frame,
                width,
                FRAME_WIDTH as usize - 1,
                FRAME_HEIGHT as usize - 1
            ),
            [4, 24, 54, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_interpolated_blended_window_background_gradient() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.window_background_gradient = {
              colors = { '#ff0000', '#00ff00', '#0000ff' },
              interpolation = 'Basis',
              blend = 'LinearRgb',
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm interpolated blended window_background_gradient config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Basis,
                blend: NativeWindowBackgroundGradientBlend::LinearRgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![
                    Color::Rgb(255, 0, 0),
                    Color::Rgb(0, 255, 0),
                    Color::Rgb(0, 0, 255),
                ],
            })
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let width = FRAME_WIDTH as usize;
        assert_eq!(
            frame_pixel_at(&frame, width, width / 2, FRAME_HEIGHT as usize - 1),
            [113, 213, 114, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_segmented_window_background_gradient() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.window_background_gradient = {
              colors = { '#ff0000', '#00ff00', '#0000ff' },
              segment_size = 5,
              segment_smoothness = 0.0,
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm segmented window_background_gradient config");
        app.set_config_overrides(overrides);
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: Some(NativeWindowBackgroundGradientSegment {
                    size: 5,
                    smoothness_millis: 0,
                }),
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![
                    Color::Rgb(255, 0, 0),
                    Color::Rgb(0, 255, 0),
                    Color::Rgb(0, 0, 255),
                ],
            })
        );
        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let width = FRAME_WIDTH as usize;
        assert_eq!(
            frame_pixel_at(&frame, width, 0, FRAME_HEIGHT as usize - 1),
            [255, 0, 0, 255]
        );
        assert_eq!(
            frame_pixel_at(&frame, width, width / 2, FRAME_HEIGHT as usize - 1),
            [0, 255, 0, 255]
        );
    }

    #[test]
    fn window_app_renders_wezterm_noisy_window_background_gradient() {
        let mut smooth_app = NativeWindowApp::new(None);
        let smooth_overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.window_background_gradient = {
              colors = { '#000000', '#ff0000' },
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected smooth WezTerm window_background_gradient config");
        smooth_app.set_config_overrides(smooth_overrides);
        let mut smooth_frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            smooth_app.render_framebuffer(&mut smooth_frame),
            FrameRenderMode::Full
        );

        let mut noisy_app = NativeWindowApp::new(None);
        let noisy_overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}

            config.window_background_gradient = {
              colors = { '#000000', '#ff0000' },
              noise = 128,
            }

            return config
            "##,
        )
        .expect("expected noisy WezTerm window_background_gradient config");
        noisy_app.set_config_overrides(noisy_overrides);
        let mut noisy_frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(
            noisy_app.render_framebuffer(&mut noisy_frame),
            FrameRenderMode::Full
        );

        let width = FRAME_WIDTH as usize;
        let y = FRAME_HEIGHT as usize - 1;
        let x = 96;
        assert_ne!(
            frame_pixel_at(&noisy_frame, width, x, y),
            frame_pixel_at(&smooth_frame, width, x, y)
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_gradient_for_window_background_gradient_colors() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.window_background_gradient = {
              colors = wezterm.color.gradient({
                colors = { '#010203', '#111213' },
              }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.gradient window_background_gradient colors");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(1, 2, 3), Color::Rgb(17, 18, 19)],
            })
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_gradient_dotted_comment_call() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.window_background_gradient = {
              colors = wezterm.color -- gradient namespace
                .gradient({
                  colors = { '#050607', '#151617' },
                }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.gradient dotted comment config");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(5, 6, 7), Color::Rgb(21, 22, 23)],
            })
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_gradient_static_alias() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local gradient = wezterm.color.gradient

            config.window_background_gradient = {
              colors = gradient({
                colors = { '#121314', '#222324' },
              }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.gradient static alias config");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(18, 19, 20), Color::Rgb(34, 35, 36)],
            })
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_gradient_static_alias_static_key_module() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wt = require 'wezterm'
            local config = {}
            local color_key = 'color'
            local gradient_key = 'gradient'
            local gradient = wt[color_key][gradient_key]

            config.window_background_gradient = {
              colors = gradient({
                colors = { '#18191a', '#28292a' },
              }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.gradient static-key module alias config");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(24, 25, 26), Color::Rgb(40, 41, 42)],
            })
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_gradient_static_alias_comment_call() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local gradient = wezterm.color.gradient

            config.window_background_gradient = {
              colors = gradient -- stops
                ({
                  colors = { '#141516', '#242526' },
                }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.gradient static alias comment config");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(20, 21, 22), Color::Rgb(36, 37, 38)],
            })
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_gradient_static_alias_dotted_comment() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local gradient = wezterm.color -- gradient namespace
              .gradient

            config.window_background_gradient = {
              colors = gradient({
                colors = { '#161718', '#262728' },
              }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.gradient static alias dotted-comment config");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(22, 23, 24), Color::Rgb(38, 39, 40)],
            })
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_static_alias_for_window_background_gradient_colors() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local parse_color = wezterm.color.parse

            config.window_background_gradient = {
              colors = {
                parse_color('#010203'),
                parse_color('rgba(17,18,19,0.5)'),
              },
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.parse window_background_gradient colors");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(1, 2, 3), Color::Rgb(17, 18, 19)],
            })
        );
    }

    #[test]
    fn window_app_parses_legacy_wezterm_gradient_colors_for_window_background_gradient_colors() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.window_background_gradient = {
              colors = wezterm.gradient_colors({
                colors = { '#202122', '#303132' },
              }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected legacy WezTerm gradient_colors window_background_gradient colors");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(32, 33, 34), Color::Rgb(48, 49, 50)],
            })
        );
    }

    #[test]
    fn window_app_parses_legacy_wezterm_gradient_colors_dotted_comment_call() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.window_background_gradient = {
              colors = wezterm -- legacy gradient helper
                .gradient_colors({
                  colors = { '#292a2b', '#393a3b' },
                }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected legacy WezTerm gradient_colors dotted comment config");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(41, 42, 43), Color::Rgb(57, 58, 59)],
            })
        );
    }

    #[test]
    fn window_app_parses_legacy_wezterm_gradient_colors_static_alias() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local gradient_colors = wezterm.gradient_colors

            config.window_background_gradient = {
              colors = gradient_colors({
                colors = { '#252627', '#353637' },
              }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected legacy WezTerm gradient_colors static alias config");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(37, 38, 39), Color::Rgb(53, 54, 55)],
            })
        );
    }

    #[test]
    fn window_app_parses_legacy_wezterm_gradient_colors_static_alias_static_key_module() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wt = require 'wezterm'
            local config = {}
            local helper_key = 'gradient_colors'
            local gradient_colors = wt[helper_key]

            config.window_background_gradient = {
              colors = gradient_colors({
                colors = { '#2a2b2c', '#3a3b3c' },
              }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected legacy WezTerm gradient_colors static-key module alias config");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(42, 43, 44), Color::Rgb(58, 59, 60)],
            })
        );
    }

    #[test]
    fn window_app_parses_legacy_wezterm_gradient_colors_static_alias_comment_call() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local gradient_colors = wezterm.gradient_colors

            config.window_background_gradient = {
              colors = gradient_colors -- stops
                ({
                  colors = { '#28292a', '#38393a' },
                }, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected legacy WezTerm gradient_colors static alias comment config");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(40, 41, 42), Color::Rgb(56, 57, 58)],
            })
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_gradient_static_variable_spec() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local project_gradient = {
              colors = { '#404142', '#505152' },
            }

            config.window_background_gradient = {
              colors = wezterm.color.gradient(project_gradient, 2),
              noise = 0,
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.gradient static variable spec");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().window_background_gradient,
            Some(NativeWindowBackgroundGradient {
                orientation: NativeWindowBackgroundGradientOrientation::Horizontal,
                interpolation: NativeWindowBackgroundGradientInterpolation::Linear,
                blend: NativeWindowBackgroundGradientBlend::Rgb,
                noise: Some(0),
                segment: None,
                preset: None,
                opacity_alpha: u8::MAX,
                blend_with_background_color: false,
                hsb: super::native_identity_hsb(),
                colors: vec![Color::Rgb(64, 65, 66), Color::Rgb(80, 81, 82)],
            })
        );
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_foreground_color_for_framebuffer() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.colors = {
              foreground = '#040506',
            }

            return config
            "##,
        )
        .expect("expected WezTerm colors.foreground config");
        app.set_config_overrides(overrides);
        app.handle_pty_output(b"A").unwrap();
        let mut frame = vec![0; usize::try_from(FRAME_WIDTH * FRAME_HEIGHT * 4).unwrap()];

        assert_eq!(app.render_framebuffer(&mut frame), FrameRenderMode::Full);

        let terminal_origin_y = usize::from(TAB_BAR_ROWS) * CELL_HEIGHT as usize;
        let uses_configured_foreground =
            (terminal_origin_y..terminal_origin_y + CELL_HEIGHT as usize).any(|y| {
                (0..CELL_WIDTH as usize)
                    .any(|x| frame_pixel_at(&frame, FRAME_WIDTH as usize, x, y) == [4, 5, 6, 255])
            });
        assert!(
            uses_configured_foreground,
            "default text foreground did not use colors.foreground"
        );
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_colors_static_variable() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local colors = {
              foreground = '#010203',
              background = '#040506',
              cursor_bg = '#070809',
            }

            config.colors = colors

            return config
            "##,
        )
        .expect("expected WezTerm colors static variable config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let colors = effective.colors.clone().expect("expected retained colors palette");
        assert_eq!(colors.foreground, Some(Color::Rgb(1, 2, 3)));
        assert_eq!(colors.background, Some(Color::Rgb(4, 5, 6)));
        assert_eq!(colors.cursor_bg, Some(Color::Rgb(7, 8, 9)));
        assert_eq!(colors.cursor_fg, None);
        assert_eq!(effective.foreground_color, Color::Rgb(1, 2, 3));
        assert_eq!(effective.background_color, Color::Rgb(4, 5, 6));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(7, 8, 9));
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_for_lua_colors_fields() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.colors = {
              foreground = wezterm.color.parse('#010203'),
              background = wezterm.color.parse('#040506'),
              cursor_bg = wezterm.color.parse('rgb(7,8,9)'),
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.parse colors config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let colors = effective.colors.clone().expect("expected retained colors palette");
        assert_eq!(colors.foreground, Some(Color::Rgb(1, 2, 3)));
        assert_eq!(colors.background, Some(Color::Rgb(4, 5, 6)));
        assert_eq!(colors.cursor_bg, Some(Color::Rgb(7, 8, 9)));
        assert_eq!(effective.foreground_color, Color::Rgb(1, 2, 3));
        assert_eq!(effective.background_color, Color::Rgb(4, 5, 6));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(7, 8, 9));
    }

    #[test]
    fn static_wezterm_color_value_evaluator_parses_constructors() {
        use wezterm_color_types::SrgbaTuple;

        let hsla_expected = SrgbaTuple::from_hsla(120.0, 1.0, 0.25, 0.5).to_string();
        for (source, marker, expected) in [
            (
                "config.colors = { foreground = wezterm.color.parse('rgba(1,2,3,0.5)') }",
                "wezterm.color.parse",
                "rgba(0.3921569% 0.7843138% 1.1764706% 50%)".to_owned(),
            ),
            (
                "config.colors = { foreground = wezterm.color.from_hsla(120, 1, 0.25, 0.5) }",
                "wezterm.color.from_hsla",
                hsla_expected,
            ),
            (
                "local wt = require 'wezterm'\nlocal color = wt.color\ncolor.parse('#010203')",
                "color.parse",
                "#010203".to_owned(),
            ),
            (
                "local color_key = 'color'\nlocal parse_key = 'parse'\n(require('wezterm'))[color_key][parse_key]('#040506')",
                "(require('wezterm'))",
                "#040506".to_owned(),
            ),
            (
                "local wt = require 'wezterm'\nlocal parse_color = wt.color.parse\nlocal text = '#070809'\nparse_color(text)",
                "parse_color(text)",
                "#070809".to_owned(),
            ),
            (
                "local wt = require 'wezterm'\nlocal from_hsla = wt['color']['from_hsla']\nfrom_hsla(120, 1, 0.25, 0.5)",
                "from_hsla(120",
                SrgbaTuple::from_hsla(120.0, 1.0, 0.25, 0.5).to_string(),
            ),
            (
                "local h = 120\nlocal saturation = 1\nlocal lightness = 0.25\nlocal alpha = 0.5\nwezterm.color.from_hsla(h, saturation, lightness, alpha)",
                "wezterm.color.from_hsla",
                SrgbaTuple::from_hsla(120.0, 1.0, 0.25, 0.5).to_string(),
            ),
        ] {
            let start = source.find(marker).unwrap();
            let static_source = super::LuaStaticSource {
                source,
                max_start: start,
            };
            let value = super::lua_static_wezterm_color_value_from_query(
                static_source,
                source[start..].trim_end_matches('}').trim_end(),
            )
            .unwrap_or_else(|| panic!("expected static Color from {source:?}"));
            assert_eq!(value.as_color().unwrap().to_string(), expected);
        }
    }

    #[test]
    fn static_wezterm_color_value_evaluator_matches_all_color_methods() {
        let base: wezterm_color_types::SrgbaTuple = "rgba(25% 50% 75% 80%)".parse().unwrap();
        let cases = [
            ("complement()", base.complement()),
            ("complement_ryb()", base.complement_ryb()),
            ("saturate(0.2)", base.saturate(0.2)),
            ("desaturate(0.2)", base.saturate(-0.2)),
            ("saturate_fixed(0.2)", base.saturate_fixed(0.2)),
            ("desaturate_fixed(0.2)", base.saturate_fixed(-0.2)),
            ("lighten(0.2)", base.lighten(0.2)),
            ("darken(0.2)", base.lighten(-0.2)),
            ("lighten_fixed(0.2)", base.lighten_fixed(0.2)),
            ("darken_fixed(0.2)", base.lighten_fixed(-0.2)),
            ("adjust_hue_fixed(45)", base.adjust_hue_fixed(45.0)),
            ("adjust_hue_fixed_ryb(45)", base.adjust_hue_fixed_ryb(45.0)),
        ];

        for (method, expected) in cases {
            let expression = format!("wezterm.color.parse('rgba(25% 50% 75% 80%)'):{method}");
            let actual = super::lua_static_wezterm_color_value_from_query(
                super::LuaStaticSource {
                    source: &expression,
                    max_start: 0,
                },
                &expression,
            )
            .unwrap_or_else(|| panic!("expected static Color from {expression:?}"));
            assert_eq!(actual.as_color(), Some(expected), "{method}");
        }

        let source = r#"
local base = wezterm.color.parse('yellow')
local transformed = base:complement_ryb():darken(0.2):saturate_fixed(0.1)
transformed
"#;
        let start = source.rfind("transformed").unwrap();
        let actual = super::lua_static_wezterm_color_value_from_query(
            super::LuaStaticSource {
                source,
                max_start: start,
            },
            &source[start..],
        )
        .expect("expected chained Color variable");
        let expected = "yellow"
            .parse::<wezterm_color_types::SrgbaTuple>()
            .unwrap()
            .complement_ryb()
            .lighten(-0.2)
            .saturate_fixed(0.1);
        assert_eq!(actual.as_color(), Some(expected));
    }

    #[test]
    fn static_wezterm_color_value_evaluator_resolves_multi_target_results() {
        let source = r#"
local base = wezterm.color.parse('yellow')
local triad_a, triad_b = base:triad()
local square_a, square_b, square_c = base:square()
local red, green, blue, alpha = base:srgba_u8()
local linear_red, linear_green, linear_blue, linear_alpha = base:linear_rgba()
local hue, saturation, lightness, hsla_alpha = base:hsla()
local lab_l, lab_a, lab_b, lab_alpha = base:laba()
"#;
        let base = "yellow".parse::<wezterm_color_types::SrgbaTuple>().unwrap();
        let (triad_a, triad_b) = base.triad();
        let (square_a, square_b, square_c) = base.square();
        let (red, green, blue, alpha) = base.to_srgb_u8();
        let linear = base.to_linear();
        let hsla = base.to_hsla();
        let laba = base.to_laba();

        let evaluate = |variable: &str| {
            super::lua_static_wezterm_color_value_from_query(
                super::LuaStaticSource {
                    source,
                    max_start: source.len(),
                },
                variable,
            )
            .unwrap_or_else(|| panic!("expected static value for {variable}"))
        };

        for (variable, expected) in [
            ("triad_a", triad_a),
            ("triad_b", triad_b),
            ("square_a", square_a),
            ("square_b", square_b),
            ("square_c", square_c),
        ] {
            assert_eq!(evaluate(variable).as_color(), Some(expected), "{variable}");
        }
        for (variable, expected) in [
            ("red", red),
            ("green", green),
            ("blue", blue),
            ("alpha", alpha),
        ] {
            assert_eq!(
                evaluate(variable),
                super::NativeStaticLuaColorValue::Integer(expected),
                "{variable}"
            );
        }
        for (variable, expected) in [
            ("linear_red", f64::from(linear.0)),
            ("linear_green", f64::from(linear.1)),
            ("linear_blue", f64::from(linear.2)),
            ("linear_alpha", f64::from(linear.3)),
            ("hue", hsla.0),
            ("saturation", hsla.1),
            ("lightness", hsla.2),
            ("hsla_alpha", hsla.3),
            ("lab_l", laba.0),
            ("lab_a", laba.1),
            ("lab_b", laba.2),
            ("lab_alpha", laba.3),
        ] {
            let super::NativeStaticLuaColorValue::Number(actual) = evaluate(variable) else {
                panic!("expected numeric value for {variable}");
            };
            assert!((actual - expected).abs() < 1e-9, "{variable}");
        }
    }

    #[test]
    fn static_wezterm_color_value_evaluator_resolves_scalar_results() {
        let source = r#"local red = wezterm.color.parse('red')
local navy = wezterm.color.parse('navy')
local ratio = red:contrast_ratio(navy)
local distance = red:delta_e(navy)
local same = red == wezterm.color.parse('red')
local different = red ~= navy
local text = tostring(red:darken(0.2))
"#;
        let red = "red".parse::<wezterm_color_types::SrgbaTuple>().unwrap();
        let navy = "navy".parse::<wezterm_color_types::SrgbaTuple>().unwrap();
        let evaluate = |variable: &str| {
            super::lua_static_wezterm_color_value_from_query(
                super::LuaStaticSource {
                    source,
                    max_start: source.len(),
                },
                variable,
            )
            .unwrap_or_else(|| panic!("expected static value for {variable}"))
        };

        assert_eq!(
            evaluate("ratio"),
            super::NativeStaticLuaColorValue::Number(red.contrast_ratio(&navy))
        );
        assert_eq!(
            evaluate("distance"),
            super::NativeStaticLuaColorValue::Number(f64::from(red.delta_e(&navy)))
        );
        assert_eq!(
            evaluate("same"),
            super::NativeStaticLuaColorValue::Bool(true)
        );
        assert_eq!(
            evaluate("different"),
            super::NativeStaticLuaColorValue::Bool(true)
        );
        assert_eq!(
            evaluate("text"),
            super::NativeStaticLuaColorValue::String(red.lighten(-0.2).to_string())
        );

        let static_source = super::LuaStaticSource {
            source,
            max_start: source.len(),
        };
        assert_eq!(
            super::parse_maybe_static_query_f64(Some(static_source), "ratio"),
            Some(red.contrast_ratio(&navy))
        );
        assert_eq!(
            super::parse_maybe_static_query_bool(Some(static_source), "same"),
            Some(true)
        );
        assert_eq!(
            super::parse_maybe_static_query_text(Some(static_source), "text"),
            Some(red.lighten(-0.2).to_string())
        );
    }

    #[test]
    fn window_app_routes_wezterm_color_objects_through_shared_consumers() {
        let mut app = NativeWindowApp::new(None);
        let source = r#"
local wezterm = require 'wezterm'
local config = {}
local base = wezterm.color.parse('yellow')
local accent = base:complement_ryb():darken(0.2)
local triad_a, triad_b = accent:triad()

config.colors = {
  foreground = base,
  background = accent,
  cursor_bg = triad_a,
  selection_bg = triad_b,
  quick_select_match_fg = { Color = accent:lighten(0.1) },
}

config.window_background_gradient = {
  colors = { base, accent },
}
config.window_frame = {
  active_titlebar_bg = accent,
}
config.integrated_title_button_color = accent

return config
"#;
        let overrides = super::native_config_overrides_from_wezterm_lua_config(source)
            .expect("expected WezTerm Color object consumers");

        let base = "yellow".parse::<wezterm_color_types::SrgbaTuple>().unwrap();
        let accent = base.complement_ryb().lighten(-0.2);
        let (triad_a, triad_b) = accent.triad();
        let terminal = |color: wezterm_color_types::SrgbaTuple| {
            let (red, green, blue, alpha) = color.to_srgb_u8();
            if alpha == u8::MAX {
                Color::Rgb(red, green, blue)
            } else {
                Color::Rgba(red, green, blue, alpha)
            }
        };
        let gradient_start = source.find("config.window_background_gradient").unwrap();
        let static_source = super::LuaStaticSource {
            source,
            max_start: gradient_start,
        };
        assert_eq!(
            super::lua_opaque_color_from_query_with_static_source(Some(static_source), "base"),
            Some(terminal(base))
        );
        assert_eq!(
            super::split_lua_table_color_expression_array_with_static_source(
                Some(static_source),
                "{ base, accent }",
            ),
            Some(vec!["base".to_owned(), "accent".to_owned()])
        );
        assert_eq!(
            overrides
                .window_background_gradient
                .as_ref()
                .expect("expected raw gradient")
                .colors,
            vec![terminal(base), terminal(accent)]
        );
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let colors = effective.colors.clone().expect("expected colors");
        assert_eq!(colors.foreground, Some(terminal(base)));
        assert_eq!(colors.background, Some(terminal(accent)));
        assert_eq!(colors.cursor_bg, Some(terminal(triad_a)));
        assert_eq!(colors.selection_bg, Some(terminal(triad_b)));
        assert_eq!(
            effective.quick_select_match_fg,
            Some(NativeColorSpec::Color(terminal(accent.lighten(0.1))))
        );
        assert_eq!(
            effective.window_frame_appearance.active_titlebar_bg,
            Some(terminal(accent))
        );
        assert_eq!(
            effective.integrated_title_button_color,
            NativeIntegratedTitleButtonColor::Color(terminal(accent))
        );

        let background_overrides = super::native_config_overrides_from_wezterm_lua_config(
            r#"
local wezterm = require 'wezterm'
local config = {}
local accent = wezterm.color.parse('yellow'):complement_ryb():darken(0.2)
config.background = {
  { source = { Color = accent } },
}
return config
"#,
        )
        .expect("expected Color object background layer");
        assert_eq!(
            background_overrides.background,
            Some(vec![super::NativeWindowBackgroundVisualLayer::Color(
                terminal(accent)
            )])
        );

        let scheme_overrides = super::native_config_overrides_from_wezterm_lua_config(
            r#"
local wezterm = require 'wezterm'
local config = {}
local accent = wezterm.color.parse('yellow'):complement_ryb():darken(0.2)
config.color_schemes = {
  Project = {
    foreground = accent,
    background = '#000000',
  },
}
config.color_schemes.Project.cursor_bg = accent:lighten(0.1)
config.color_scheme = 'Project'
return config
"#,
        )
        .expect("expected Color object custom scheme mutation");
        let mut scheme_app = NativeWindowApp::new(None);
        scheme_app.set_config_overrides(scheme_overrides);
        let scheme_effective = scheme_app.native_effective_config();
        assert_eq!(scheme_effective.foreground_color, terminal(accent));
        assert_eq!(
            scheme_effective.cursor_bg_color,
            terminal(accent.lighten(0.1))
        );
    }

    #[test]
    fn static_wezterm_color_value_evaluator_rejects_unprovable_forms() {
        for expression in [
            "wezterm.color.parse()",
            "wezterm.color.parse('red', 'blue')",
            "wezterm.color.from_hsla(0, 1, 0.5)",
            "wezterm.color.from_hsla(0, 1, 0.5, 1 / 0)",
            "wezterm.color.parse('red'):complement(1)",
            "wezterm.color.parse('red'):saturate()",
            "wezterm.color.parse('red').complement()",
            "wezterm.color.parse('red'):contrast_ratio(dynamic_color)",
            "wezterm.color.parse('red') == wezterm.color.parse('red') == wezterm.color.parse('red')",
        ] {
            assert!(
                super::lua_static_wezterm_color_value_from_query(
                    super::LuaStaticSource {
                        source: expression,
                        max_start: 0,
                    },
                    expression,
                )
                .is_none(),
                "unexpectedly accepted {expression:?}"
            );
        }

        for (source, query) in [
            (
                "local c = wezterm.color.parse('red')\nc = dynamic()\nc",
                "c",
            ),
            (
                "local base = wezterm.color.parse('red')\nlocal method = base.complement\nmethod()",
                "method()",
            ),
            (
                "local base = wezterm.color.parse('red')\nlocal a, b, missing = base:triad()\nmissing",
                "missing",
            ),
            (
                "local base = wezterm.color.parse('red')\nlocal a, b = base\na",
                "a",
            ),
            (
                "wezterm.color.parse = dynamic\nwezterm.color.parse('red')",
                "wezterm.color.parse('red')",
            ),
            (
                "local c = wezterm.color.parse('red')\nc.extra = dynamic\nc",
                "c",
            ),
            (
                "local c = wezterm.color.parse('red')\nlocal alias = c\nc.extra = dynamic\nalias",
                "alias",
            ),
            ("local a = b\nlocal b = a\na", "a"),
            (
                "if dynamic then\n  local c = wezterm.color.parse('red')\nend\nc",
                "c",
            ),
        ] {
            let start = source.rfind(query).unwrap();
            assert!(
                super::lua_static_wezterm_color_value_from_query(
                    super::LuaStaticSource {
                        source,
                        max_start: start,
                    },
                    &source[start..],
                )
                .is_none(),
                "unexpectedly accepted source {source:?}"
            );
        }

        let tuple = "wezterm.color.parse('red'):triad()";
        let static_source = super::LuaStaticSource {
            source: tuple,
            max_start: 0,
        };
        assert!(super::lua_static_color_number_from_query(static_source, tuple).is_none());
        assert!(super::lua_static_color_bool_from_query(static_source, tuple).is_none());
        assert!(super::lua_static_color_string_from_query(static_source, tuple).is_none());
        assert!(
            super::lua_color_from_query_with_static_source(Some(static_source), tuple).is_none()
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_dotted_comment_for_lua_colors_fields() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.colors = {
              foreground = wezterm.color -- foreground parser
                .parse('#0a0b0c'),
              background = wezterm -- background namespace
                .color.parse('#0d0e0f'),
              cursor_bg = wezterm.color -- cursor parser
                .parse('rgb(16,17,18)'),
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.parse dotted comment colors config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let colors = effective.colors.clone().expect("expected retained colors palette");
        assert_eq!(colors.foreground, Some(Color::Rgb(10, 11, 12)));
        assert_eq!(colors.background, Some(Color::Rgb(13, 14, 15)));
        assert_eq!(colors.cursor_bg, Some(Color::Rgb(16, 17, 18)));
        assert_eq!(effective.foreground_color, Color::Rgb(10, 11, 12));
        assert_eq!(effective.background_color, Color::Rgb(13, 14, 15));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(16, 17, 18));
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_static_alias_for_lua_colors_fields() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local parse_color = wezterm.color.parse

            config.colors = {
              foreground = parse_color('#101112'),
              background = parse_color('#131415'),
              cursor_bg = parse_color('rgb(22,23,24)'),
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.parse static alias colors config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let colors = effective.colors.clone().expect("expected retained colors palette");
        assert_eq!(colors.foreground, Some(Color::Rgb(16, 17, 18)));
        assert_eq!(colors.background, Some(Color::Rgb(19, 20, 21)));
        assert_eq!(colors.cursor_bg, Some(Color::Rgb(22, 23, 24)));
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(19, 20, 21));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(22, 23, 24));
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_static_alias_static_key_module() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wt = require 'wezterm'
            local config = {}
            local color_key = 'color'
            local parse_key = 'parse'
            local parse_color = wt[color_key][parse_key]

            config.colors = {
              foreground = parse_color('#252627'),
              background = parse_color('#28292a'),
              cursor_bg = parse_color('rgb(43,44,45)'),
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.parse static-key module alias colors config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let colors = effective.colors.clone().expect("expected retained colors palette");
        assert_eq!(colors.foreground, Some(Color::Rgb(37, 38, 39)));
        assert_eq!(colors.background, Some(Color::Rgb(40, 41, 42)));
        assert_eq!(colors.cursor_bg, Some(Color::Rgb(43, 44, 45)));
        assert_eq!(effective.foreground_color, Color::Rgb(37, 38, 39));
        assert_eq!(effective.background_color, Color::Rgb(40, 41, 42));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(43, 44, 45));
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_static_alias_dotted_comment() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local parse_color = wezterm.color -- parser namespace
              .parse

            config.colors = {
              foreground = parse_color('#111213'),
              background = parse_color('#141516'),
              cursor_bg = parse_color('rgb(23,24,25)'),
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.parse dotted-comment static alias colors config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let colors = effective.colors.clone().expect("expected retained colors palette");
        assert_eq!(colors.foreground, Some(Color::Rgb(17, 18, 19)));
        assert_eq!(colors.background, Some(Color::Rgb(20, 21, 22)));
        assert_eq!(colors.cursor_bg, Some(Color::Rgb(23, 24, 25)));
        assert_eq!(effective.foreground_color, Color::Rgb(17, 18, 19));
        assert_eq!(effective.background_color, Color::Rgb(20, 21, 22));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(23, 24, 25));
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_static_alias_comment_call() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local parse_color = wezterm.color.parse

            config.colors = {
              foreground = parse_color -- foreground
                ('#202122'),
              background = parse_color -- background
                ('#232425'),
              cursor_bg = parse_color -- cursor
                ('rgb(38,39,40)'),
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.parse static alias comment colors config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let colors = effective.colors.clone().expect("expected retained colors palette");
        assert_eq!(colors.foreground, Some(Color::Rgb(32, 33, 34)));
        assert_eq!(colors.background, Some(Color::Rgb(35, 36, 37)));
        assert_eq!(colors.cursor_bg, Some(Color::Rgb(38, 39, 40)));
        assert_eq!(effective.foreground_color, Color::Rgb(32, 33, 34));
        assert_eq!(effective.background_color, Color::Rgb(35, 36, 37));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(38, 39, 40));
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_static_alias_for_lua_palette_and_color_spec_fields() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local parse_color = wezterm.color.parse

            config.colors = {
              ansi = {
                parse_color('#010203'), parse_color('#040506'),
                parse_color('#070809'), parse_color('#0a0b0c'),
                parse_color('#0d0e0f'), parse_color('#101112'),
                parse_color('#131415'), parse_color('#161718'),
              },
              brights = {
                parse_color('#191a1b'), parse_color('#1c1d1e'),
                parse_color('#1f2021'), parse_color('#222324'),
                parse_color('#252627'), parse_color('#28292a'),
                parse_color('#2b2c2d'), parse_color('#2e2f30'),
              },
              indexed = {
                [16] = parse_color('#313233'),
                [136] = parse_color('rgb(64,65,66)'),
              },
              copy_mode_active_highlight_bg = { Color = parse_color('#434445') },
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.parse static alias palette config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let palette = effective.ansi_palette.expect("expected ANSI palette");
        assert_eq!(palette[0], Color::Rgb(1, 2, 3));
        assert_eq!(palette[7], Color::Rgb(22, 23, 24));
        assert_eq!(palette[8], Color::Rgb(25, 26, 27));
        assert_eq!(palette[15], Color::Rgb(46, 47, 48));
        let indexed = effective.indexed_palette.expect("expected indexed palette");
        assert_eq!(indexed[16], Some(Color::Rgb(49, 50, 51)));
        assert_eq!(indexed[136], Some(Color::Rgb(64, 65, 66)));
        assert_eq!(
            effective.copy_mode_active_highlight_bg,
            Some(NativeColorSpec::Color(Color::Rgb(67, 68, 69)))
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_static_alias_for_lua_tab_bar_and_visual_bell_colors() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local parse_color = wezterm.color.parse

            config.colors = {
              tab_bar = {
                background = parse_color('#010203'),
                inactive_tab_edge = parse_color('#040506'),
                active_tab = {
                  bg_color = parse_color('#070809'),
                  fg_color = parse_color('#0a0b0c'),
                },
              },
              visual_bell = parse_color('#0d0e0f'),
            }

            return config
            "##,
        )
        .expect("expected WezTerm color.parse static alias tab_bar config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(
            effective.tab_bar_background_color,
            Some(Color::Rgb(1, 2, 3))
        );
        assert_eq!(
            effective.tab_bar_inactive_tab_edge_color,
            Some(Color::Rgb(4, 5, 6))
        );
        assert_eq!(
            effective.tab_bar_active_tab_colors.bg_color,
            Some(Color::Rgb(7, 8, 9))
        );
        assert_eq!(
            effective.tab_bar_active_tab_colors.fg_color,
            Some(Color::Rgb(10, 11, 12))
        );
        assert_eq!(effective.visual_bell_color, Some(Color::Rgb(13, 14, 15)));
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_static_alias_for_lua_color_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local parse_color = wezterm.color.parse
            local colors = {}

            colors.ansi = {}
            colors.ansi[2] = parse_color('#010203')
            colors.brights = {}
            colors.brights[8] = parse_color('#040506')
            colors.tab_bar = {}
            colors.tab_bar.background = parse_color('#070809')
            colors.tab_bar.active_tab = {}
            colors.tab_bar.active_tab.bg_color = parse_color('#0a0b0c')
            colors.tab_bar.active_tab.fg_color = parse_color('#0d0e0f')
            config.colors = colors

            return config
            "##,
        )
        .expect("expected WezTerm color.parse static alias color mutations");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let palette = effective.ansi_palette.expect("expected ANSI palette");
        assert_eq!(palette[1], Color::Rgb(1, 2, 3));
        assert_eq!(palette[15], Color::Rgb(4, 5, 6));
        assert_eq!(
            effective.tab_bar_background_color,
            Some(Color::Rgb(7, 8, 9))
        );
        assert_eq!(
            effective.tab_bar_active_tab_colors.bg_color,
            Some(Color::Rgb(10, 11, 12))
        );
        assert_eq!(
            effective.tab_bar_active_tab_colors.fg_color,
            Some(Color::Rgb(13, 14, 15))
        );
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_custom_color_scheme() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#010203',
                background = '#040506',
                ansi = {
                  '#000000',
                  '#111213',
                  '#141516',
                  '#171819',
                  '#1a1b1c',
                  '#1d1e1f',
                  '#202122',
                  '#232425',
                },
                brights = {
                  '#262728',
                  '#292a2b',
                  '#2c2d2e',
                  '#2f3031',
                  '#323334',
                  '#353637',
                  '#38393a',
                  '#3b3c3d',
                },
                indexed = {
                  [136] = '#070809',
                },
              },
            }

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.color_scheme, Some("Project Scheme".to_owned()));
        let scheme = effective
            .color_schemes
            .get("Project Scheme")
            .expect("expected retained Project Scheme");
        assert_eq!(scheme.foreground, Color::Rgb(1, 2, 3));
        assert_eq!(scheme.background, Color::Rgb(4, 5, 6));
        assert_eq!(scheme.ansi[1], Color::Rgb(17, 18, 19));
        assert_eq!(scheme.brights[1], Color::Rgb(41, 42, 43));
        assert_eq!(scheme.indexed[136], Some(Color::Rgb(7, 8, 9)));
        assert_eq!(effective.foreground_color, Color::Rgb(1, 2, 3));
        assert_eq!(effective.background_color, Color::Rgb(4, 5, 6));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[1],
            Color::Rgb(17, 18, 19)
        );
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[9],
            Color::Rgb(41, 42, 43)
        );
        assert_eq!(
            effective.indexed_palette.expect("expected indexed palette")[136],
            Some(Color::Rgb(7, 8, 9))
        );

        let resolved = effective.resolved_palette;
        assert_eq!(resolved.foreground, Color::Rgb(1, 2, 3));
        assert_eq!(resolved.background, Color::Rgb(4, 5, 6));
        assert_eq!(resolved.ansi[1], Color::Rgb(17, 18, 19));
        assert_eq!(resolved.brights[1], Color::Rgb(41, 42, 43));
        assert_eq!(resolved.indexed[136], Some(Color::Rgb(7, 8, 9)));
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_custom_color_scheme_from_static_variable() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local project_scheme = {
              foreground = '#101112',
              background = '#131415',
              cursor_bg = '#161718',
            }

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = project_scheme,
            }

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme variable config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.color_scheme, Some("Project Scheme".to_owned()));
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(19, 20, 21));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(22, 23, 24));
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_color_scheme_static_string_variable() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local selected_scheme = 'Project Scheme'

            config.color_scheme = selected_scheme
            config.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#101112',
                background = '#131415',
                cursor_bg = '#161718',
              },
            }

            return config
            "##,
        )
        .expect("expected WezTerm color_scheme static string variable config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.color_scheme, Some("Project Scheme".to_owned()));
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(19, 20, 21));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(22, 23, 24));
    }

    #[test]
    fn window_app_applies_wezterm_lua_config_custom_color_scheme_static_variable_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local project_scheme = {
              foreground = '#101112',
              background = '#131415',
              cursor_bg = '#161718',
            }
            project_scheme.background = '#353637'
            project_scheme['cursor_bg'] = '#38393a'

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = project_scheme,
            }

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme static variable mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(53, 54, 55));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(56, 57, 58));
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_custom_color_schemes_static_variable() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local project_schemes = {
              ['Project Scheme'] = {
                foreground = '#101112',
                background = '#131415',
                cursor_bg = '#161718',
              },
            }

            config.color_scheme = 'Project Scheme'
            config.color_schemes = project_schemes

            return config
            "##,
        )
        .expect("expected WezTerm color_schemes static variable config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(19, 20, 21));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(22, 23, 24));
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_custom_color_schemes_static_variable_entries() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local project_schemes = {}

            project_schemes['Project Scheme'] = {
              foreground = '#101112',
              background = '#131415',
              cursor_bg = '#161718',
            }

            config.color_scheme = 'Project Scheme'
            config.color_schemes = project_schemes

            return config
            "##,
        )
        .expect("expected WezTerm color_schemes static variable entry config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(19, 20, 21));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(22, 23, 24));
    }

    #[test]
    fn window_app_applies_wezterm_lua_config_custom_color_schemes_static_variable_entry_mutations()
    {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local project_schemes = {}

            project_schemes['Project Scheme'] = {
              foreground = '#101112',
              background = '#131415',
              cursor_bg = '#161718',
            }
            project_schemes['Project Scheme'].background = '#353637'
            project_schemes['Project Scheme']['cursor_bg'] = '#38393a'

            config.color_scheme = 'Project Scheme'
            config.color_schemes = project_schemes

            return config
            "##,
        )
        .expect("expected WezTerm color_schemes static variable entry mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(53, 54, 55));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(56, 57, 58));
    }

    #[test]
    fn window_app_ignores_wezterm_lua_config_helper_static_color_scheme_variable_assignments() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local project_scheme

            local function ignored()
              project_scheme = {
                foreground = '#010203',
                background = '#040506',
              }
            end

            project_scheme = {
              foreground = '#101112',
              background = '#131415',
            }

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = project_scheme,
            }

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme variable config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(19, 20, 21));
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_custom_color_scheme_bracket_assignment() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local project_scheme = {
              foreground = '#202122',
              background = '#232425',
              cursor_bg = '#262728',
            }

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {}
            config.color_schemes['Project Scheme'] = project_scheme

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme bracket assignment config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(32, 33, 34));
        assert_eq!(effective.background_color, Color::Rgb(35, 36, 37));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(38, 39, 40));
    }

    #[test]
    fn window_app_parses_wezterm_lua_config_custom_color_scheme_dot_assignment() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local project_scheme = {
              foreground = '#3b3c3d',
              background = '#3e3f40',
              cursor_bg = '#414243',
            }

            config.color_scheme = 'ProjectScheme'
            config.color_schemes = {}
            config.color_schemes.ProjectScheme = project_scheme

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme dot assignment config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(59, 60, 61));
        assert_eq!(effective.background_color, Color::Rgb(62, 63, 64));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(65, 66, 67));
    }

    #[test]
    fn window_app_ignores_dynamic_wezterm_lua_color_scheme_bracket_assignment() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local dynamic_name = 'Other Scheme'

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#292a2b',
                background = '#2c2d2e',
              },
            }
            config.color_schemes[dynamic_name] = {
              foreground = '#010203',
              background = '#040506',
            }

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme config with ignored dynamic assignment");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(41, 42, 43));
        assert_eq!(effective.background_color, Color::Rgb(44, 45, 46));
    }

    #[test]
    fn window_app_uses_later_wezterm_lua_color_scheme_table_entry() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#010203',
                background = '#040506',
              },
              ['Project Scheme'] = {
                foreground = '#2f3031',
                background = '#323334',
              },
            }

            return config
            "##,
        )
        .expect("expected duplicate WezTerm custom color scheme table entry config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(47, 48, 49));
        assert_eq!(effective.background_color, Color::Rgb(50, 51, 52));
    }

    #[test]
    fn window_app_uses_later_wezterm_lua_color_scheme_bracket_assignment() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#010203',
                background = '#040506',
              },
            }
            config.color_schemes['Project Scheme'] = {
              foreground = '#353637',
              background = '#38393a',
            }

            return config
            "##,
        )
        .expect("expected overriding WezTerm custom color scheme bracket assignment config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(53, 54, 55));
        assert_eq!(effective.background_color, Color::Rgb(56, 57, 58));
    }

    #[test]
    fn window_app_ignores_wezterm_lua_config_helper_color_scheme_assignments() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {}
            config.color_schemes['Project Scheme'] = {
              foreground = '#202122',
              background = '#232425',
            }

            local function ignored()
              config.color_schemes['Project Scheme'] = {
                foreground = '#010203',
                background = '#040506',
              }
            end

            return config
            "##,
        )
        .expect("expected WezTerm helper color scheme assignment config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(32, 33, 34));
        assert_eq!(effective.background_color, Color::Rgb(35, 36, 37));
    }

    #[test]
    fn window_app_uses_returned_config_variable_later_color_scheme_bracket_assignment() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local cfg = {}

            config.color_scheme = 'Ignored Scheme'
            config.color_schemes = {
              ['Ignored Scheme'] = {
                foreground = '#010203',
                background = '#040506',
              },
            }

            cfg.color_scheme = 'Project Scheme'
            cfg.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#070809',
                background = '#0a0b0c',
              },
            }
            cfg.color_schemes['Project Scheme'] = {
              foreground = '#353637',
              background = '#38393a',
            }

            return cfg
            "##,
        )
        .expect("expected returned config variable custom color scheme bracket assignment");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(53, 54, 55));
        assert_eq!(effective.background_color, Color::Rgb(56, 57, 58));
    }

    #[test]
    fn window_app_applies_returned_config_variable_color_scheme_entry_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local cfg = {}

            config.color_scheme = 'Ignored Scheme'
            config.color_schemes = {
              ['Ignored Scheme'] = {
                foreground = '#010203',
                background = '#040506',
              },
            }
            config.color_schemes['Ignored Scheme'].background = '#070809'

            cfg.color_scheme = 'Project Scheme'
            cfg.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#101112',
                background = '#131415',
                cursor_bg = '#161718',
              },
            }
            cfg.color_schemes['Project Scheme'].background = '#353637'
            cfg.color_schemes['Project Scheme']['cursor_bg'] = '#38393a'

            return cfg
            "##,
        )
        .expect("expected returned config variable custom color scheme mutations");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(53, 54, 55));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(56, 57, 58));
    }

    #[test]
    fn window_app_uses_later_wezterm_lua_color_scheme_assignment_after_entry_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {}
            config.color_schemes['Project Scheme'] = {
              foreground = '#010203',
              background = '#040506',
              cursor_bg = '#070809',
            }
            config.color_schemes['Project Scheme'].background = '#0a0b0c'
            config.color_schemes['Project Scheme'].cursor_bg = '#0d0e0f'
            config.color_schemes['Project Scheme'] = {
              foreground = '#101112',
              background = '#131415',
              cursor_bg = '#161718',
            }

            return config
            "##,
        )
        .expect("expected later WezTerm custom color scheme assignment config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(16, 17, 18));
        assert_eq!(effective.background_color, Color::Rgb(19, 20, 21));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(22, 23, 24));
    }

    #[test]
    fn window_app_applies_wezterm_lua_custom_color_scheme_entry_static_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#010203',
                background = '#040506',
                cursor_bg = '#070809',
              },
            }
            config.color_schemes['Project Scheme'].background = '#0a0b0c'
            config.color_schemes['Project Scheme']['cursor_bg'] = '#0d0e0f'

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme entry mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(1, 2, 3));
        assert_eq!(effective.background_color, Color::Rgb(10, 11, 12));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(13, 14, 15));
    }

    #[test]
    fn window_app_ignores_wezterm_lua_config_helper_color_scheme_entry_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#313233',
                background = '#343536',
                cursor_bg = '#373839',
              },
            }

            local function ignored()
              config.color_schemes['Project Scheme'].background = '#010203'
              config.color_schemes['Project Scheme'].cursor_bg = '#040506'
            end

            return config
            "##,
        )
        .expect("expected WezTerm helper color scheme entry mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(49, 50, 51));
        assert_eq!(effective.background_color, Color::Rgb(52, 53, 54));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(55, 56, 57));
    }

    #[test]
    fn window_app_applies_wezterm_lua_custom_color_scheme_entry_palette_slot_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                ansi = {
                  '#000001',
                  '#000002',
                  '#000003',
                  '#000004',
                  '#000005',
                  '#000006',
                  '#000007',
                  '#000008',
                },
                brights = {
                  '#000009',
                  '#00000a',
                  '#00000b',
                  '#00000c',
                  '#00000d',
                  '#00000e',
                  '#00000f',
                  '#000010',
                },
              },
            }
            config.color_schemes['Project Scheme'].ansi[2] = '#101112'
            config.color_schemes['Project Scheme'].brights[8] = '#131415'

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme entry palette slot mutation config");
        app.set_config_overrides(overrides);

        let palette = app
            .native_effective_config()
            .ansi_palette
            .expect("expected ANSI palette");
        assert_eq!(palette[0], Color::Rgb(0, 0, 1));
        assert_eq!(palette[1], Color::Rgb(16, 17, 18));
        assert_eq!(palette[8], Color::Rgb(0, 0, 9));
        assert_eq!(palette[15], Color::Rgb(19, 20, 21));
    }

    #[test]
    fn window_app_applies_wezterm_lua_custom_color_scheme_entry_indexed_slot_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                indexed = {
                  [136] = '#010203',
                },
              },
            }
            config.color_schemes['Project Scheme'].indexed[137] = '#040506'

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme entry indexed slot mutation config");
        app.set_config_overrides(overrides);

        let indexed = app
            .native_effective_config()
            .indexed_palette
            .expect("expected indexed palette");
        assert_eq!(indexed[136], Some(Color::Rgb(1, 2, 3)));
        assert_eq!(indexed[137], Some(Color::Rgb(4, 5, 6)));
    }

    #[test]
    fn window_app_applies_wezterm_lua_custom_color_scheme_entry_tab_bar_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                tab_bar = {
                  background = '#010203',
                  active_tab = {
                    fg_color = '#040506',
                    bg_color = '#070809',
                    intensity = 'Normal',
                  },
                },
              },
            }
            config.color_schemes['Project Scheme'].tab_bar.background = '#0a0b0c'
            config.color_schemes['Project Scheme'].tab_bar.active_tab.bg_color = '#0d0e0f'
            config.color_schemes['Project Scheme'].tab_bar.active_tab.intensity = 'Bold'

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme entry tab-bar mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(
            effective.tab_bar_background_color,
            Some(Color::Rgb(10, 11, 12))
        );
        assert_eq!(
            effective.tab_bar_active_tab_colors.fg_color,
            Some(Color::Rgb(4, 5, 6))
        );
        assert_eq!(
            effective.tab_bar_active_tab_colors.bg_color,
            Some(Color::Rgb(13, 14, 15))
        );
        assert_eq!(
            effective.tab_bar_active_tab_colors.intensity,
            Some(NativeFormatIntensity::Bold)
        );
    }

    #[test]
    fn window_app_parses_wezterm_color_parse_static_alias_for_lua_color_scheme_entry_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local parse_color = wezterm.color.parse

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                foreground = '#202122',
                ansi = {
                  '#000001',
                  '#000002',
                  '#000003',
                  '#000004',
                  '#000005',
                  '#000006',
                  '#000007',
                  '#000008',
                },
                brights = {
                  '#000009',
                  '#00000a',
                  '#00000b',
                  '#00000c',
                  '#00000d',
                  '#00000e',
                  '#00000f',
                  '#000010',
                },
                indexed = {
                  [136] = '#232425',
                },
                tab_bar = {
                  background = '#262728',
                  active_tab = {
                    fg_color = '#292a2b',
                    bg_color = '#2c2d2e',
                  },
                },
              },
            }
            config.color_schemes['Project Scheme'].foreground = parse_color('#010203')
            config.color_schemes['Project Scheme'].ansi[2] = parse_color('#040506')
            config.color_schemes['Project Scheme'].brights[8] = parse_color('#070809')
            config.color_schemes['Project Scheme'].indexed[137] = parse_color('#0a0b0c')
            config.color_schemes['Project Scheme'].tab_bar.background = parse_color('#0d0e0f')
            config.color_schemes['Project Scheme'].tab_bar.active_tab.bg_color = parse_color('#101112')
            config.color_schemes['Project Scheme'].tab_bar.active_tab.fg_color = parse_color('#131415')

            return config
            "##,
        )
        .expect("expected WezTerm color.parse static alias color_scheme entry mutations");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(1, 2, 3));
        let palette = effective.ansi_palette.expect("expected ANSI palette");
        assert_eq!(palette[1], Color::Rgb(4, 5, 6));
        assert_eq!(palette[15], Color::Rgb(7, 8, 9));
        let indexed = effective.indexed_palette.expect("expected indexed palette");
        assert_eq!(indexed[137], Some(Color::Rgb(10, 11, 12)));
        assert_eq!(
            effective.tab_bar_background_color,
            Some(Color::Rgb(13, 14, 15))
        );
        assert_eq!(
            effective.tab_bar_active_tab_colors.bg_color,
            Some(Color::Rgb(16, 17, 18))
        );
        assert_eq!(
            effective.tab_bar_active_tab_colors.fg_color,
            Some(Color::Rgb(19, 20, 21))
        );
    }

    #[test]
    fn window_app_parses_wezterm_lua_color_scheme_entry_color_spec_nested_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local parse_color = wezterm.color.parse

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {
              ['Project Scheme'] = {
                copy_mode_active_highlight_bg = { Color = '#202122' },
                copy_mode_active_highlight_fg = { AnsiColor = 'White' },
                quick_select_match_bg = { AnsiColor = 'Black' },
                quick_select_match_fg = { Color = '#232425' },
              },
            }
            config.color_schemes['Project Scheme'].copy_mode_active_highlight_bg.Color = parse_color('#010203')
            config.color_schemes['Project Scheme'].copy_mode_active_highlight_fg.AnsiColor = 'Black'
            config.color_schemes['Project Scheme'].quick_select_match_bg.AnsiColor = 'Navy'
            config.color_schemes['Project Scheme'].quick_select_match_fg.Color = parse_color('#040506')

            return config
            "##,
        )
        .expect("expected WezTerm color scheme entry nested color spec mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(
            effective.copy_mode_active_highlight_bg,
            Some(NativeColorSpec::Color(Color::Rgb(1, 2, 3)))
        );
        assert_eq!(
            effective.copy_mode_active_highlight_fg,
            Some(NativeColorSpec::AnsiColor(NativeAnsiColor::Black))
        );
        assert_eq!(
            effective.quick_select_match_bg,
            Some(NativeColorSpec::AnsiColor(NativeAnsiColor::Navy))
        );
        assert_eq!(
            effective.quick_select_match_fg,
            Some(NativeColorSpec::Color(Color::Rgb(4, 5, 6)))
        );
    }

    #[test]
    fn window_app_parses_wezterm_lua_custom_color_scheme_from_load_scheme() {
        static NEXT_COLOR_SCHEME_LOAD_SCHEME_ID: AtomicUsize = AtomicUsize::new(0);

        let mut scheme_file = std::env::temp_dir();
        scheme_file.push(format!(
            "rssh-color-scheme-load-scheme-{}-{}.toml",
            std::process::id(),
            NEXT_COLOR_SCHEME_LOAD_SCHEME_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&scheme_file);
        std::fs::write(
            &scheme_file,
            r##"
            [metadata]
            name = "Loaded Scheme"

            [colors]
            foreground = "#3b3c3d"
            background = "#3e3f40"
            cursor_bg = "#414243"
            ansi = [
              "#000001",
              "#000002",
              "#000003",
              "#000004",
              "#000005",
              "#000006",
              "#000007",
              "#000008",
            ]
            "##,
        )
        .expect("expected temp custom color_scheme load_scheme TOML scheme");
        let scheme_file_query = scheme_file.to_string_lossy().replace('\\', "/");

        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local wezterm = require 'wezterm'
            local config = {{}}

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {{
              ['Project Scheme'] = wezterm.color.load_scheme('{scheme_file_query}'),
            }}

            return config
            "##
        ))
        .expect("expected WezTerm custom color_scheme load_scheme config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(59, 60, 61));
        assert_eq!(effective.background_color, Color::Rgb(62, 63, 64));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(65, 66, 67));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[1],
            Color::Rgb(0, 0, 2)
        );
        let _ = std::fs::remove_file(scheme_file);
    }

    #[test]
    fn window_app_parses_wezterm_lua_custom_color_scheme_from_load_scheme_alias() {
        static NEXT_COLOR_SCHEME_LOAD_SCHEME_ALIAS_ID: AtomicUsize = AtomicUsize::new(0);

        let mut scheme_file = std::env::temp_dir();
        scheme_file.push(format!(
            "rssh-color-scheme-load-scheme-alias-{}-{}.toml",
            std::process::id(),
            NEXT_COLOR_SCHEME_LOAD_SCHEME_ALIAS_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&scheme_file);
        std::fs::write(
            &scheme_file,
            r##"
            [metadata]
            name = "Loaded Scheme"

            [colors]
            foreground = "#454647"
            background = "#48494a"
            cursor_bg = "#4b4c4d"
            "##,
        )
        .expect("expected temp custom color_scheme load_scheme alias TOML scheme");
        let scheme_file_query = scheme_file.to_string_lossy().replace('\\', "/");

        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local wezterm = require 'wezterm'
            local config = {{}}
            local load_scheme = wezterm.color.load_scheme

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {{
              ['Project Scheme'] = load_scheme('{scheme_file_query}'),
            }}

            return config
            "##
        ))
        .expect("expected WezTerm custom color_scheme load_scheme alias config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(69, 70, 71));
        assert_eq!(effective.background_color, Color::Rgb(72, 73, 74));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(75, 76, 77));
        let _ = std::fs::remove_file(scheme_file);
    }

    #[test]
    fn window_app_parses_wezterm_lua_custom_color_scheme_from_load_scheme_static_path_expression() {
        static NEXT_COLOR_SCHEME_LOAD_SCHEME_STATIC_PATH_ID: AtomicUsize = AtomicUsize::new(0);

        let mut scheme_file = std::env::temp_dir();
        scheme_file.push(format!(
            "rssh-color-scheme-load-scheme-static-path-{}-{}.toml",
            std::process::id(),
            NEXT_COLOR_SCHEME_LOAD_SCHEME_STATIC_PATH_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&scheme_file);
        std::fs::write(
            &scheme_file,
            r##"
            [metadata]
            name = "Static Path Loaded Scheme"

            [colors]
            foreground = "#616263"
            background = "#646566"
            cursor_bg = "#676869"
            ansi = [
              "#000001",
              "#000002",
              "#000003",
              "#000004",
              "#000005",
              "#000006",
              "#000007",
              "#000008",
            ]
            "##,
        )
        .expect("expected temp custom color_scheme static-path TOML scheme");
        let scheme_file_query = scheme_file.to_string_lossy().replace('\\', "/");
        let scheme_name = scheme_file
            .file_name()
            .expect("expected static-path scheme filename")
            .to_string_lossy()
            .into_owned();
        let scheme_dir = scheme_file_query
            .strip_suffix(&scheme_name)
            .expect("expected normalized scheme path to end with its filename");
        assert!(scheme_dir.ends_with('/'));

        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local wt = require 'wezterm'
            local config = {{}}
            local load_scheme = wt.color.load_scheme
            local scheme_dir = '{scheme_dir}'
            local scheme_name = '{scheme_name}'
            local scheme_path = scheme_dir .. scheme_name

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {{
              ['Project Scheme'] = load_scheme(scheme_path),
            }}

            return config
            "##
        ))
        .expect("expected WezTerm custom color_scheme static-path load_scheme config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(97, 98, 99));
        assert_eq!(effective.background_color, Color::Rgb(100, 101, 102));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(103, 104, 105));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[3],
            Color::Rgb(0, 0, 4)
        );
        let _ = std::fs::remove_file(scheme_file);
    }

    #[test]
    fn window_app_parses_wezterm_lua_custom_color_scheme_from_load_scheme_wezterm_config_dir() {
        static NEXT_COLOR_SCHEME_LOAD_SCHEME_WEZTERM_CONFIG_DIR_ID: AtomicUsize =
            AtomicUsize::new(0);

        let Some(original_config_dir) = std::env::var_os("WEZTERM_CONFIG_DIR") else {
            return;
        };
        let scheme_dir = PathBuf::from(&original_config_dir);
        if std::fs::create_dir_all(&scheme_dir).is_err() {
            return;
        }
        let mut scheme_file = scheme_dir.clone();
        scheme_file.push(format!(
            "rssh-color-scheme-load-scheme-wezterm-config-dir-{}-{}.toml",
            std::process::id(),
            NEXT_COLOR_SCHEME_LOAD_SCHEME_WEZTERM_CONFIG_DIR_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&scheme_file);
        std::fs::write(
            &scheme_file,
            r##"
            [metadata]
            name = "Config Dir Loaded Scheme"

            [colors]
            foreground = "#c1c2c3"
            background = "#c4c5c6"
            cursor_bg = "#c7c8c9"
            ansi = [
              "#000001",
              "#000002",
              "#000003",
              "#000004",
              "#000005",
              "#000006",
              "#000007",
              "#000008",
            ]
            "##,
        )
        .expect("expected temp custom color_scheme config_dir-load-scheme TOML file");
        let scheme_name = scheme_file
            .file_name()
            .expect("expected temp scheme file name")
            .to_string_lossy()
            .into_owned();

        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local wt = require 'wezterm'
            local config = {{}}
            local load_scheme = wt.color.load_scheme

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {{
              ['Project Scheme'] = load_scheme(wt.config_dir .. '/{scheme_name}'),
            }}

            return config
            "##
        ))
        .expect("expected WezTerm custom color_scheme config_dir load_scheme config");

        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(193, 194, 195));
        assert_eq!(effective.background_color, Color::Rgb(196, 197, 198));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(199, 200, 201));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[5],
            Color::Rgb(0, 0, 5)
        );
        let _ = std::fs::remove_file(scheme_file);
    }

    #[test]
    fn window_app_parses_wezterm_lua_custom_color_scheme_from_load_scheme_variable() {
        static NEXT_COLOR_SCHEME_LOAD_SCHEME_VARIABLE_ID: AtomicUsize = AtomicUsize::new(0);

        let mut scheme_file = std::env::temp_dir();
        scheme_file.push(format!(
            "rssh-color-scheme-load-scheme-variable-{}-{}.toml",
            std::process::id(),
            NEXT_COLOR_SCHEME_LOAD_SCHEME_VARIABLE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&scheme_file);
        std::fs::write(
            &scheme_file,
            r##"
            [metadata]
            name = "Loaded Scheme"

            [colors]
            foreground = "#444546"
            background = "#474849"
            cursor_bg = "#4a4b4c"
            "##,
        )
        .expect("expected temp custom color_scheme variable load_scheme TOML scheme");
        let scheme_file_query = scheme_file.to_string_lossy().replace('\\', "/");

        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local wezterm = require 'wezterm'
            local config = {{}}
            local project_scheme = wezterm.color.load_scheme('{scheme_file_query}')

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {{
              ['Project Scheme'] = project_scheme,
            }}

            return config
            "##
        ))
        .expect("expected WezTerm custom color_scheme variable load_scheme config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(68, 69, 70));
        assert_eq!(effective.background_color, Color::Rgb(71, 72, 73));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(74, 75, 76));
        let _ = std::fs::remove_file(scheme_file);
    }

    #[test]
    fn window_app_applies_wezterm_lua_custom_color_scheme_load_scheme_variable_mutations() {
        static NEXT_COLOR_SCHEME_LOAD_SCHEME_VARIABLE_MUTATION_ID: AtomicUsize =
            AtomicUsize::new(0);

        let mut scheme_file = std::env::temp_dir();
        scheme_file.push(format!(
            "rssh-color-scheme-load-scheme-variable-mutation-{}-{}.toml",
            std::process::id(),
            NEXT_COLOR_SCHEME_LOAD_SCHEME_VARIABLE_MUTATION_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&scheme_file);
        std::fs::write(
            &scheme_file,
            r##"
            [metadata]
            name = "Loaded Scheme"

            [colors]
            foreground = "#4d4e4f"
            background = "#505152"
            cursor_bg = "#535455"
            "##,
        )
        .expect("expected temp custom color_scheme variable mutation TOML scheme");
        let scheme_file_query = scheme_file.to_string_lossy().replace('\\', "/");

        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(&format!(
            r##"
            local wezterm = require 'wezterm'
            local config = {{}}
            local project_scheme = wezterm.color.load_scheme('{scheme_file_query}')
            project_scheme.background = '#565758'

            config.color_scheme = 'Project Scheme'
            config.color_schemes = {{
              ['Project Scheme'] = project_scheme,
            }}

            return config
            "##
        ))
        .expect("expected WezTerm custom color_scheme load_scheme variable mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(77, 78, 79));
        assert_eq!(effective.background_color, Color::Rgb(86, 87, 88));
        assert_eq!(effective.cursor_bg_color, Color::Rgb(83, 84, 85));
        let _ = std::fs::remove_file(scheme_file);
    }

    #[test]
    fn window_app_parses_wezterm_lua_custom_color_scheme_from_builtin_lookup() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local scheme = wezterm.color.get_builtin_schemes()['Gruvbox Light']
            scheme.background = '#010203'

            config.color_scheme = 'Gruvbox Light'
            config.color_schemes = {
              ['Gruvbox Light'] = scheme,
              ['Gruvbox Custom'] = scheme,
            }

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme builtin lookup config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.color_scheme, Some("Gruvbox Light".to_owned()));
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(1, 2, 3));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[1],
            Color::Rgb(157, 0, 6)
        );
        let scheme = effective
            .color_schemes
            .get("Gruvbox Custom")
            .expect("expected Gruvbox Custom custom scheme");
        assert_eq!(scheme.foreground, Color::Rgb(40, 40, 40));
        assert_eq!(scheme.background, Color::Rgb(1, 2, 3));
    }

    #[test]
    fn window_app_parses_wezterm_lua_builtin_scheme_from_whole_map_variable() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local scheme_name = 'Gruvbox Light'
            local schemes = wezterm.color.get_builtin_schemes()
            config.colors = schemes[scheme_name]
            return config
            "##,
        )
        .expect("expected whole-map built-in color scheme lookup config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
    }

    #[test]
    fn window_app_applies_wezterm_lua_builtin_scheme_whole_map_lookup_to_inline_custom_color_schemes()
     {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            config.color_scheme = 'Mine'

            return config
            "##,
        )
        .expect("expected whole-map built-in inline custom color scheme config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
        let palette = effective.ansi_palette.expect("expected ANSI palette");
        assert_eq!(palette[1], Color::Rgb(157, 0, 6));
        assert_eq!(palette[8], Color::Rgb(157, 131, 116));
    }

    #[test]
    fn window_app_applies_wezterm_lua_builtin_scheme_whole_map_lookup_to_direct_custom_color_scheme_assignment()
     {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            config.color_schemes['Mine'] = schemes['Gruvbox Light']
            config.color_scheme = 'Mine'

            return config
            "##,
        )
        .expect("expected whole-map built-in direct custom color scheme config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
        let palette = effective.ansi_palette.expect("expected ANSI palette");
        assert_eq!(palette[1], Color::Rgb(157, 0, 6));
        assert_eq!(palette[8], Color::Rgb(157, 131, 116));
    }

    #[test]
    fn window_app_rejects_mutated_builtin_scheme_whole_map_in_custom_color_schemes() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            schemes['Gruvbox Light'] = choose_palette()
            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "a dynamically mutated built-in scheme map must fail closed"
        );
    }

    #[test]
    fn window_app_rejects_escaped_builtin_scheme_whole_map_in_custom_color_schemes() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            mutate(schemes)
            config.color_schemes['Mine'] = schemes['Gruvbox Light']
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "a built-in scheme map passed to an unknown call must fail closed"
        );
    }

    #[test]
    fn window_app_rejects_aliased_builtin_scheme_whole_map_in_custom_color_schemes() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            local alias = schemes
            alias['Gruvbox Light'] = choose_palette()
            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "an aliased built-in scheme map must fail closed"
        );
    }

    #[test]
    fn window_app_rejects_called_closure_capturing_builtin_scheme_whole_map() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            local function mutate_later()
              schemes['Gruvbox Light'] = choose_palette()
            end
            mutate_later()
            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "calling a closure that captures the built-in scheme map must fail closed"
        );
    }

    #[test]
    fn window_app_rejects_function_entry_write_to_builtin_scheme_whole_map() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            schemes['Gruvbox Light'] = function() end
            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "a function-valued built-in scheme map entry write must fail closed"
        );
    }

    #[test]
    fn window_app_rejects_builtin_scheme_whole_map_method_escape_in_custom_color_schemes() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            schemes:mutate()
            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "calling a method on the built-in scheme map must fail closed"
        );
    }

    #[test]
    fn window_app_ignores_uncalled_function_body_builtin_scheme_map_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            local function mutate_later()
              schemes['Gruvbox Light'] = choose_palette()
            end
            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            config.color_scheme = 'Mine'

            return config
            "##,
        )
        .expect("expected uncalled function body not to mutate the built-in scheme map");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
    }

    #[test]
    fn window_app_rejects_builtin_scheme_map_escape_before_same_statement_lookup() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            local function mutate(map)
              map['Gruvbox Light'].background = '#010203'
            end
            local schemes = wezterm.color.get_builtin_schemes()
            config.color_schemes = {
              mutate(schemes),
              ['Mine'] = schemes['Gruvbox Light'],
            }
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "a built-in scheme map escape before a same-statement lookup must fail closed"
        );
    }

    #[test]
    fn window_app_rejects_post_lookup_builtin_scheme_map_nested_mutation() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            schemes['Gruvbox Light'].background = '#010203'
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "a post-lookup nested built-in scheme map mutation must fail closed"
        );
    }

    #[test]
    fn window_app_preserves_builtin_scheme_map_lookup_across_later_rebind() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            schemes = wezterm.color.get_builtin_schemes()
            schemes['Gruvbox Light'].background = '#010203'
            config.color_scheme = 'Mine'

            return config
            "##,
        )
        .expect("expected later map rebind not to change the captured built-in scheme");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
    }

    #[test]
    fn window_app_resolves_multiple_inline_builtin_scheme_map_lookups() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            config.color_schemes = {
              ['A'] = schemes['Gruvbox Light'],
              ['B'] = schemes['Builtin Solarized Dark'],
            }
            config.color_scheme = 'A'

            return config
            "##,
        )
        .expect("expected multiple inline built-in scheme map lookups");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
    }

    #[test]
    fn window_app_resolves_multiple_direct_builtin_scheme_map_lookups() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            config.color_schemes['A'] = schemes['Gruvbox Light']
            config.color_schemes['B'] = schemes['Builtin Solarized Dark']
            config.color_scheme = 'A'

            return config
            "##,
        )
        .expect("expected multiple direct built-in scheme map lookups");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
    }

    #[test]
    fn window_app_rejects_builtin_scheme_map_rebound_from_config_alias() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            schemes = config.color_schemes
            schemes.Mine.background = '#010203'
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "rebinding from config.color_schemes must not prove a fresh map"
        );
    }

    #[test]
    fn window_app_rejects_builtin_scheme_map_rebound_from_dynamic_call() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()

            config.color_schemes = {
              ['Mine'] = schemes['Gruvbox Light'],
            }
            schemes = choose_map()
            config.color_scheme = 'Mine'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "a dynamic call must not prove a fresh built-in scheme map"
        );
    }

    #[test]
    fn window_app_rejects_builtin_scheme_map_lookup_with_closure_rebound_static_key() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local scheme_name = 'Gruvbox Light'
            local function switch_scheme()
              scheme_name = 'Builtin Solarized Dark'
            end
            local schemes = wezterm.color.get_builtin_schemes()

            switch_scheme()
            config.color_schemes['A'] = schemes[scheme_name]
            config.color_scheme = 'A'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "a closure-rebound static scheme key must fail closed"
        );
    }

    #[test]
    fn window_app_rejects_builtin_scheme_map_lookup_after_inline_static_key_mutation() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local scheme_name = 'Gruvbox Light'
            local schemes = wezterm.color.get_builtin_schemes()

            config.color_schemes = {
              (function()
                scheme_name = 'Builtin Solarized Dark'
              end)(),
              ['A'] = schemes[scheme_name],
            }
            config.color_scheme = 'A'

            return config
            "##,
        );

        assert!(
            overrides.is_none(),
            "an inline static scheme key mutation must fail closed"
        );
    }

    #[test]
    fn window_app_applies_wezterm_lua_builtin_scheme_lookup_to_inline_custom_color_schemes() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_scheme = 'Gruvbox Custom'
            config.color_schemes = {
              ['Gruvbox Custom'] = wezterm.color.get_builtin_schemes()['Gruvbox Light'],
            }

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme builtin inline lookup config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.color_scheme, Some("Gruvbox Custom".to_owned()));
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[8],
            Color::Rgb(157, 131, 116)
        );
        assert_eq!(
            effective
                .color_schemes
                .get("Gruvbox Custom")
                .expect("expected Gruvbox Custom custom scheme")
                .brights[7],
            Color::Rgb(124, 111, 100)
        );
    }

    #[test]
    fn window_app_applies_wezterm_lua_builtin_scheme_lookup_to_direct_custom_color_scheme_assignment()
     {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wt = require 'wezterm'
            local config = {}
            local get_schemes = wt.get_builtin_color_schemes

            config.color_scheme = 'Legacy Gruvbox'
            config.color_schemes['Legacy Gruvbox'] = get_schemes()['Gruvbox Light']

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme builtin direct assignment config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.color_scheme, Some("Legacy Gruvbox".to_owned()));
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
        let scheme = effective
            .color_schemes
            .get("Legacy Gruvbox")
            .expect("expected Legacy Gruvbox custom scheme");
        assert_eq!(scheme.ansi[1], Color::Rgb(157, 0, 6));
    }

    #[test]
    fn window_app_applies_wezterm_lua_builtin_scheme_lookup_to_custom_color_scheme_entry_mutations()
    {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}

            config.color_schemes = {
              ['Gruvbox Custom'] = wezterm.color.get_builtin_schemes()['Gruvbox Light'],
            }
            config.color_schemes['Gruvbox Custom'].foreground = '#7f7f7f'
            config.color_schemes['Gruvbox Custom'].ansi[1] = '#0f1011'
            config.color_scheme = 'Gruvbox Custom'

            return config
            "##,
        )
        .expect("expected WezTerm custom color scheme entry mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        let scheme = effective
            .color_schemes
            .get("Gruvbox Custom")
            .expect("expected Gruvbox Custom custom scheme");
        assert_eq!(scheme.foreground, Color::Rgb(127, 127, 127));
        assert_eq!(scheme.ansi[0], Color::Rgb(15, 16, 17));
        assert_eq!(effective.foreground_color, Color::Rgb(127, 127, 127));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[0],
            Color::Rgb(15, 16, 17)
        );
    }

    #[test]
    fn window_app_applies_wezterm_lua_builtin_scheme_map_palette_variable_mutations_to_custom_color_scheme()
     {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes['Gruvbox Light']

            scheme.background = '#010203'
            config.color_schemes = { ['Mine'] = scheme }
            config.color_scheme = 'Mine'

            return config
            "##,
        )
        .expect("expected WezTerm built-in scheme map palette variable mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(1, 2, 3));
        let ansi = effective.ansi_palette.expect("expected ANSI palette");
        assert_eq!(ansi[0], Color::Rgb(251, 241, 199));
        assert_eq!(ansi[8], Color::Rgb(157, 131, 116));
    }

    #[test]
    fn window_app_applies_legacy_builtin_scheme_map_function_alias_palette_variable_mutations() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wt = require 'wezterm'
            local config = {}
            local get_schemes = wt.get_builtin_color_schemes
            local schemes = get_schemes()
            local scheme = schemes['Gruvbox Light']

            scheme.background = '#010203'
            config.color_schemes = { ['Mine'] = scheme }
            config.color_scheme = 'Mine'

            return config
            "##,
        )
        .expect("expected legacy built-in scheme map function alias palette mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(1, 2, 3));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[1],
            Color::Rgb(157, 0, 6)
        );
    }

    #[test]
    fn window_app_captures_builtin_scheme_map_palette_variable_static_key_at_lookup() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local scheme_name = 'Gruvbox Light'
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes[scheme_name]

            scheme_name = 'Builtin Solarized Dark'
            scheme.background = '#010203'
            config.color_schemes = { ['Mine'] = scheme }
            config.color_scheme = 'Mine'

            return config
            "##,
        )
        .expect("expected static scheme key to be captured at palette lookup");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(1, 2, 3));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[8],
            Color::Rgb(157, 131, 116)
        );
    }

    #[test]
    fn window_app_rejects_unprovable_builtin_scheme_map_palette_variable_bindings() {
        for (label, source) in [
            (
                "dynamic map rebind before lookup",
                r##"
                local wezterm = require 'wezterm'
                local config = {}
                local schemes = wezterm.color.get_builtin_schemes()
                schemes = choose_map()
                local scheme = schemes['Gruvbox Light']

                config.color_schemes = { ['Mine'] = scheme }
                config.color_scheme = 'Mine'
                return config
                "##,
            ),
            (
                "dynamic key",
                r##"
                local wezterm = require 'wezterm'
                local config = {}
                local scheme_name = choose_scheme()
                local schemes = wezterm.color.get_builtin_schemes()
                local scheme = schemes[scheme_name]

                config.color_schemes = { ['Mine'] = scheme }
                config.color_scheme = 'Mine'
                return config
                "##,
            ),
            (
                "closure-rebound key",
                r##"
                local wezterm = require 'wezterm'
                local config = {}
                local scheme_name = 'Gruvbox Light'
                local function switch_scheme()
                  scheme_name = 'Builtin Solarized Dark'
                end
                local schemes = wezterm.color.get_builtin_schemes()

                switch_scheme()
                local scheme = schemes[scheme_name]
                config.color_schemes = { ['Mine'] = scheme }
                config.color_scheme = 'Mine'
                return config
                "##,
            ),
            (
                "helper-local map",
                r##"
                local wezterm = require 'wezterm'
                local config = {}
                local function make_scheme()
                  local schemes = wezterm.color.get_builtin_schemes()
                  return schemes['Gruvbox Light']
                end
                local scheme = make_scheme()

                config.color_schemes = { ['Mine'] = scheme }
                config.color_scheme = 'Mine'
                return config
                "##,
            ),
        ] {
            assert!(
                super::native_config_overrides_from_wezterm_lua_config(source).is_none(),
                "{label} must fail closed"
            );
        }
    }

    #[test]
    fn window_app_preserves_builtin_scheme_map_palette_clone_across_later_map_rebind() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes['Gruvbox Light']

            scheme.background = '#010203'
            schemes = wezterm.color.get_builtin_schemes()
            schemes['Gruvbox Light'].background = '#aabbcc'
            config.color_schemes = { ['Mine'] = scheme }
            config.color_scheme = 'Mine'

            return config
            "##,
        )
        .expect("expected palette clone to survive a later built-in scheme map rebind");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(1, 2, 3));
        assert_eq!(
            effective.ansi_palette.expect("expected ANSI palette")[0],
            Color::Rgb(251, 241, 199)
        );
    }

    #[test]
    fn window_app_rejects_escaped_builtin_scheme_map_palette_variable_bindings() {
        for (label, statement) in [
            ("unknown rebind", "scheme = choose_palette()"),
            ("unknown call argument", "mutate(scheme)"),
            ("unknown call receiver", "scheme:mutate()"),
            (
                "alias escape",
                "local alias = scheme\nalias.background = '#010203'",
            ),
            (
                "called capturing closure",
                "local function mutate_later()\n  scheme.background = '#010203'\nend\nmutate_later()",
            ),
        ] {
            let source = format!(
                r##"
                local wezterm = require 'wezterm'
                local config = {{}}
                local schemes = wezterm.color.get_builtin_schemes()
                local scheme = schemes['Gruvbox Light']

                {statement}
                config.color_schemes = {{ ['Mine'] = scheme }}
                config.color_scheme = 'Mine'
                return config
                "##,
            );

            assert!(
                super::native_config_overrides_from_wezterm_lua_config(&source).is_none(),
                "{label} must fail closed"
            );
        }
    }

    #[test]
    fn window_app_rejects_called_palette_capture_after_known_rebind() {
        for (label, calls) in [
            (
                "capture called after known rebind",
                "scheme = schemes['Builtin Solarized Dark']\nmutate_later()",
            ),
            (
                "capture called before and after known rebind",
                "mutate_later()\nscheme = schemes['Builtin Solarized Dark']\nmutate_later()",
            ),
        ] {
            let source = format!(
                r##"
                local wezterm = require 'wezterm'
                local config = {{}}
                local schemes = wezterm.color.get_builtin_schemes()
                local scheme = schemes['Gruvbox Light']
                local function mutate_later()
                  scheme.background = '#010203'
                end

                {calls}
                config.color_schemes = {{ ['Mine'] = scheme }}
                config.color_scheme = 'Mine'
                return config
                "##,
            );

            assert!(
                super::native_config_overrides_from_wezterm_lua_config(&source).is_none(),
                "{label} must fail closed"
            );
        }
    }

    #[test]
    fn window_app_rejects_palette_mutation_rhs_identity_reads_and_escapes() {
        for (label, mutation) in [
            (
                "unknown call receives palette identity",
                "scheme.background = mutate(scheme)",
            ),
            (
                "mutation RHS reads palette identity",
                "scheme.background = scheme.foreground",
            ),
            (
                "literal-prefixed RHS calls with palette identity",
                "scheme.background = '#010203' .. mutate(scheme)",
            ),
            (
                "literal-prefixed RHS reads palette identity",
                "scheme.background = '#010203' .. scheme.foreground",
            ),
        ] {
            let source = format!(
                r##"
                local wezterm = require 'wezterm'
                local config = {{}}
                local schemes = wezterm.color.get_builtin_schemes()
                local scheme = schemes['Gruvbox Light']

                {mutation}
                config.color_schemes = {{ ['Mine'] = scheme }}
                config.color_scheme = 'Mine'
                return config
                "##,
            );

            assert!(
                super::native_config_overrides_from_wezterm_lua_config(&source).is_none(),
                "{label} must fail closed"
            );
        }
    }

    #[test]
    fn window_app_rejects_aliased_palette_capture_after_known_rebind() {
        let source = r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes['Gruvbox Light']
            local function mutate_original()
              scheme.background = '#010203'
            end
            local mutate = mutate_original
            scheme = schemes['Builtin Solarized Dark']
            mutate()
            config.colors = scheme
            return config
            "##;

        assert!(
            super::native_config_overrides_from_wezterm_lua_config(source).is_none(),
            "a called alias of a captured closure must fail closed after palette rebind"
        );
    }

    #[test]
    fn window_app_rejects_old_palette_capture_alias_after_function_redefinition() {
        let source = r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes['Gruvbox Light']
            local function mutate()
              scheme.background = '#010203'
            end
            local old_mutate = mutate
            local function mutate()
              return true
            end
            scheme = schemes['Builtin Solarized Dark']
            old_mutate()
            config.colors = scheme
            return config
            "##;

        assert!(
            super::native_config_overrides_from_wezterm_lua_config(source).is_none(),
            "an alias must retain the captured closure after its original name is redefined"
        );
    }

    #[test]
    fn window_app_does_not_treat_predeclaration_global_capture_as_later_local_palette_capture() {
        let mut app = NativeWindowApp::new(None);
        let source = r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local function mutate_global()
              scheme.background = '#010203'
            end
            local scheme = schemes['Builtin Solarized Dark']
            mutate_global()
            config.colors = scheme
            return config
            "##;

        let overrides = super::native_config_overrides_from_wezterm_lua_config(source)
            .expect("a function declared before the local palette must capture the global cell");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().background_color,
            Color::Rgb(0, 43, 54)
        );
    }

    #[test]
    fn window_app_rejects_palette_boolean_literal_expression_tail() {
        let source = r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes['Gruvbox Light']
            scheme.tab_bar.active_tab.italic = false or true
            config.colors = scheme
            return config
            "##;

        assert!(
            super::native_config_overrides_from_wezterm_lua_config(source).is_none(),
            "a boolean prefix followed by an expression tail must fail closed"
        );
    }

    #[test]
    fn window_app_replaces_empty_indexed_palette_before_slot_patch() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}
            local scheme = {
              foreground = '#101112',
              background = '#131415',
              indexed = { [136] = '#010203' },
            }
            scheme.indexed = {}
            scheme.indexed[137] = '#040506'
            config.colors = scheme
            return config
            "##,
        )
        .expect("expected empty indexed replacement followed by a slot patch");
        app.set_config_overrides(overrides);

        let indexed = app
            .native_effective_config()
            .indexed_palette
            .expect("expected an indexed palette after the slot patch");
        assert_eq!(indexed[136], None);
        assert_eq!(indexed[137], Some(Color::Rgb(4, 5, 6)));
    }

    #[test]
    fn window_app_reduces_palette_whole_replacements_and_slot_patches_in_source_order() {
        for (label, mutations, expected) in [
            (
                "whole replacement wins last",
                r##"
                scheme.ansi[1] = '#010203'
                scheme.ansi = {
                  '#101112', '#131415', '#161718', '#191a1b',
                  '#1c1d1e', '#1f2021', '#222324', '#252627',
                }
                "##,
                Color::Rgb(16, 17, 18),
            ),
            (
                "slot patch wins last",
                r##"
                scheme.ansi = {
                  '#101112', '#131415', '#161718', '#191a1b',
                  '#1c1d1e', '#1f2021', '#222324', '#252627',
                }
                scheme.ansi[1] = '#010203'
                "##,
                Color::Rgb(1, 2, 3),
            ),
        ] {
            let source = format!(
                r##"
                local config = {{}}
                local scheme = {{
                  foreground = '#101112',
                  background = '#131415',
                }}
                {mutations}
                config.colors = scheme
                return config
                "##,
            );
            let mut app = NativeWindowApp::new(None);
            let overrides = super::native_config_overrides_from_wezterm_lua_config(&source)
                .unwrap_or_else(|| panic!("expected ordered palette mutations: {label}"));
            app.set_config_overrides(overrides);

            let palette = app
                .native_effective_config()
                .ansi_palette
                .expect("expected an ANSI palette");
            assert_eq!(palette[0], expected, "{label}");
        }
    }

    #[test]
    fn window_app_rejects_unfinished_empty_palette_composites() {
        for (label, mutation) in [
            ("ansi", "scheme.ansi = {}"),
            ("brights", "scheme.brights = {}"),
            ("color spec", "scheme.copy_mode_active_highlight_bg = {}"),
        ] {
            let source = format!(
                r##"
                local wezterm = require 'wezterm'
                local config = {{}}
                local schemes = wezterm.color.get_builtin_schemes()
                local scheme = schemes['Gruvbox Light']
                {mutation}
                config.colors = scheme
                return config
                "##,
            );

            assert!(
                super::native_config_overrides_from_wezterm_lua_config(&source).is_none(),
                "unfinished empty {label} replacement must fail closed"
            );
        }
    }

    #[test]
    fn window_app_clears_tab_bar_fields_on_whole_replacement() {
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local config = {}
            local scheme = {
              foreground = '#101112',
              background = '#131415',
              tab_bar = {
                background = '#010203',
                active_tab = {
                  bg_color = '#040506',
                  fg_color = '#070809',
                },
              },
            }
            scheme.tab_bar = {}
            config.colors = scheme
            return config
            "##,
        )
        .expect("expected an empty tab-bar replacement");

        assert_eq!(overrides.tab_bar_background_color, None);
        assert_eq!(
            overrides.tab_bar_active_tab_colors,
            NativeTabBarItemColors::default()
        );
    }

    #[test]
    fn split_lua_top_level_arguments_preserves_long_string_starting_with_open_bracket() {
        let arguments = super::split_lua_top_level_arguments("[[[foo]], 'after'")
            .expect("expected a valid long-string argument list");

        assert_eq!(arguments, vec!["[[[foo]]", " 'after'"]);
    }

    #[test]
    fn window_app_applies_builtin_palette_variable_long_bracket_mutation() {
        let mut app = NativeWindowApp::new(None);
        let source = r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes['Gruvbox Light']

            scheme[[[background]]] = '#010203'
            config.color_schemes = { ['Mine'] = scheme }
            config.color_scheme = 'Mine'
            return config
            "##;

        let overrides = super::native_config_overrides_from_wezterm_lua_config(source)
            .expect("expected long-bracket palette variable mutation config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(1, 2, 3));
    }

    #[test]
    fn window_app_applies_builtin_palette_variable_repeated_field_mutations_in_order() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes['Gruvbox Light']

            scheme.background = '#010203'
            scheme.background = '#040506'
            config.color_schemes = { ['Mine'] = scheme }
            config.color_scheme = 'Mine'
            return config
            "##,
        )
        .expect("expected ordered repeated palette variable mutations");
        app.set_config_overrides(overrides);

        assert_eq!(
            app.native_effective_config().background_color,
            Color::Rgb(4, 5, 6)
        );
    }

    #[test]
    fn window_app_applies_palette_color_spec_replacement_in_order() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes['Gruvbox Light']

            scheme.copy_mode_active_highlight_bg.Color = '#010203'
            scheme.copy_mode_active_highlight_bg = { AnsiColor = 'Navy' }
            scheme.quick_select_match_fg = { AnsiColor = 'Black' }
            scheme.quick_select_match_fg.Color = '#040506'
            config.color_schemes = { ['Mine'] = scheme }
            config.color_scheme = 'Mine'
            return config
            "##,
        )
        .expect("expected ordered palette color spec replacement config");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(
            effective.copy_mode_active_highlight_bg,
            Some(NativeColorSpec::AnsiColor(NativeAnsiColor::Navy))
        );
        assert_eq!(
            effective.quick_select_match_fg,
            Some(NativeColorSpec::Color(Color::Rgb(4, 5, 6)))
        );
    }

    #[test]
    fn window_app_ignores_palette_mutations_before_builtin_scheme_map_variable_binding() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local scheme = {}
            scheme.background = '#010203'
            local schemes = wezterm.color.get_builtin_schemes()
            scheme = schemes['Gruvbox Light']

            config.color_schemes = { ['Mine'] = scheme }
            config.color_scheme = 'Mine'
            return config
            "##,
        )
        .expect("expected the latest built-in palette variable binding");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(40, 40, 40));
        assert_eq!(effective.background_color, Color::Rgb(251, 241, 199));
    }

    #[test]
    fn window_app_resets_palette_mutations_after_known_builtin_scheme_map_variable_rebind() {
        let mut app = NativeWindowApp::new(None);
        let overrides = super::native_config_overrides_from_wezterm_lua_config(
            r##"
            local wezterm = require 'wezterm'
            local config = {}
            local schemes = wezterm.color.get_builtin_schemes()
            local scheme = schemes['Gruvbox Light']
            scheme.background = '#010203'
            scheme = choose_palette()
            scheme = schemes['Builtin Solarized Dark']

            config.color_schemes = { ['Mine'] = scheme }
            config.color_scheme = 'Mine'
            return config
            "##,
        )
        .expect("expected the latest known built-in palette variable binding");
        app.set_config_overrides(overrides);

        let effective = app.native_effective_config();
        assert_eq!(effective.foreground_color, Color::Rgb(131, 148, 150));
        assert_eq!(effective.background_color, Color::Rgb(0, 43, 54));
    }

    #[test]
    fn window_app_loads_every_config_manifest_builtin_color_scheme() {
        let names = rssh_config::schemes::names().collect::<Vec<_>>();
        assert_eq!(names.len(), 1_113, "pinned built-in scheme manifest size");

        for name in names {
            let mut expected = NativeConfigSnapshot::default();
            assert_eq!(
                super::apply_builtin_color_scheme_overrides(name, &mut expected),
                Some(true),
                "load canonical palette for {name:?}"
            );
            let actual = super::native_config_overrides_from_wezterm_lua_config(&format!(
                "return {{ color_scheme = [=[{name}]=] }}"
            ))
            .unwrap_or_else(|| panic!("parse Lua built-in scheme selection for {name:?}"));

            assert_eq!(
                actual.color_scheme.as_deref(),
                Some(name),
                "selected scheme name for {name:?}"
            );
            assert_eq!(
                super::native_palette_from_overrides(&actual),
                super::native_palette_from_overrides(&expected),
                "effective palette for {name:?}"
            );
        }
    }

///
/// Heavily inspired by (and code "borrowed" from): https://observablehq.com/@johnowhitaker/dotswarm-exploring-slime-mould-inspired-shaders
///
use crate::preset::{Preset, PresetName};
use chrono::Local;
use glium::glutin::event::{ElementState, Event, StartCause, VirtualKeyCode, WindowEvent};
use glium::glutin::event_loop::{ControlFlow, EventLoop};
use glium::glutin::window::Fullscreen;
use glium::{glutin, Surface};
use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::{Duration, Instant};

pub mod beat;
pub mod config;
pub mod midi;
pub mod preset;
pub mod screenshot;
pub mod shader_pipeline;
pub mod slime_mould;

fn main() {
    let app_config = config::get_config();
    let midi_channel = midi::MidiChannel::new(app_config.midi_device_id);

    let mut beat_detector = beat::BeatDetector::new();

    let (beat_sender, beat_receiver) = sync_channel(64);

    if app_config.audio_host_name.is_some() && app_config.audio_device_id.is_some() {
        beat_detector.start_listening(
            app_config.audio_host_name.unwrap(),
            app_config.audio_device_id.unwrap(),
            move |(_, bpm)| {
                beat_sender.send(bpm).unwrap();
            },
        );
    };

    // 1. The **winit::EventsLoop** for handling events.
    let event_loop = glutin::event_loop::EventLoop::new();

    let monitor = event_loop.primary_monitor().unwrap();
    let monitor_size = monitor.size();

    let (width, height) = (monitor_size.width, monitor_size.height);

    // 2. Parameters for building the Window.
    let wb = glutin::window::WindowBuilder::new()
        .with_inner_size(glutin::dpi::LogicalSize::new(width as f32, height as f32))
        .with_title("Hello world!")
        .with_fullscreen(Some(glutin::window::Fullscreen::Borderless(
            event_loop.primary_monitor(),
        )));

    // 3. Parameters for building the OpenGL context.
    let cb = glutin::ContextBuilder::new().with_depth_buffer(24);

    // 4. Build the Display with the given window and OpenGL context parameters
    //    and register the window with the event_loop.
    let display = glium::Display::new(wb, cb, &event_loop).unwrap();

    // Start out with a randomized preset
    let preset = rand::random();

    // Create our slime mould simulation
    let mut slime_mould = slime_mould::SlimeMould::new(&display, width, height, preset);

    let mut fullscreen = false;

    let mut screenshot_taker = screenshot::AsyncScreenshotTaker::new(5);

    let mut beat_preset: preset::Preset = rand::random();
    let mut non_beat_preset = preset;

    let mut u_time: f32 = 0.0;
    let mut u_time_takeover = false;
    let mut beat_start_time = u_time;
    let primary_window_id = display.gl_window().window().id();
    start_loop(event_loop, move |events| {
        screenshot_taker.next_frame();

        let mut got_beat = false;
        for _bpm in beat_receiver.try_iter() {
            got_beat = true;
            //println!("Got beat! BPM: {bpm:.2}");
        }

        let mut target = display.draw();
        target.clear_color(0.0, 0.0, 0.0, 1.0);
        slime_mould.draw(&mut target, &display, u_time);
        target.finish().unwrap();

        if !u_time_takeover {
            u_time += 0.02;
        }
        slime_mould.update();

        let mut action = Action::Continue;

        let mut randomize_beat_preset = false;
        let mut toggle_fullscreen = false;
        let mut stop_event_loop = false;
        let mut randomize_preset = false;
        let mut regenerate_points = false;
        let mut backspace_pressed = false;
        let mut clear_textures = false;
        let mut save_preset = false;

        let mut load_preset_number = -1;
        let mut load_beat_preset_number = -1;

        for event in events {
            if let Event::WindowEvent { event, window_id } = event {
                if *window_id == primary_window_id {
                    match event {
                        WindowEvent::CloseRequested => action = Action::Stop,
                        WindowEvent::KeyboardInput { input, .. } => {
                            if let ElementState::Pressed = input.state {
                                match input.virtual_keycode {
                                    Some(VirtualKeyCode::Escape) => stop_event_loop = true,
                                    Some(VirtualKeyCode::Return) => toggle_fullscreen = true,
                                    Some(VirtualKeyCode::R) => randomize_preset = true,
                                    Some(VirtualKeyCode::P) => regenerate_points = true,
                                    Some(VirtualKeyCode::C) => clear_textures = true,
                                    Some(VirtualKeyCode::S) => save_preset = true,
                                    Some(VirtualKeyCode::Back) => backspace_pressed = true,
                                    _ => (),
                                }
                                // If we received a number
                                if input.scancode >= 2 && input.scancode <= 11 {
                                    load_preset_number = ((input.scancode - 1) % 10) as i32;
                                }
                            }
                        }
                        _ => (),
                    }
                }
            }
        }

        // Midi receiver
        for m in midi_channel.try_iter() {
            println!("{m:?}");
            match m {
                midi::Mpd218Message::PadPressed(pad, _velocity, _) => {
                    if pad <= 9 {
                        load_preset_number = pad as i32;
                    } else if pad >= 16 && pad <= 25 {
                        load_beat_preset_number = (pad - 16) as i32;
                    } else {
                        match pad {
                            10 => clear_textures = true,
                            11 => regenerate_points = true,
                            12 => randomize_preset = true,
                            13 => randomize_beat_preset = true,
                            _ => (),
                        }
                    }
                }
                midi::Mpd218Message::KnobChanged(knob, value, _) => {
                    if knob == 0 {
                        u_time = value as f32 / 127.0;
                        //println!("value: {value} u_time: {u_time}");
                        u_time_takeover = true;
                    }
                }
                _ => (),
            }
        }

        // Update presets
        if randomize_beat_preset {
            beat_preset = rand::random();
        }

        // Random preset
        if randomize_preset {
            slime_mould.transition_preset(slime_mould.get_preset(), rand::random(), u_time, 1.0);
            u_time_takeover = false;
        }

        // Regenerate points
        if regenerate_points {
            slime_mould.reset_points();
        }

        if clear_textures {
            // Clear the textures and buffers
            slime_mould.clear();
        }

        if load_beat_preset_number >= 0 {
            beat_preset = Preset::new(PresetName::from_u32(load_beat_preset_number as u32));
        }

        if load_preset_number >= 0 {
            // Load presets
            slime_mould.transition_preset(
                slime_mould.get_preset(),
                Preset::new(PresetName::from_u32(load_preset_number as u32)),
                u_time,
                1.0,
            );
            slime_mould.reset_points();
            u_time_takeover = false;
        }

        if save_preset {
            slime_mould.save_preset();
        }

        // /*
        if got_beat {
            beat_start_time = u_time;
            non_beat_preset = slime_mould.get_preset();
            slime_mould.transition_preset(non_beat_preset, beat_preset, u_time, 0.2);
        } else {
            if beat_start_time > 0.0 {
                if (u_time - beat_start_time) > 0.2 {
                    slime_mould.transition_preset(beat_preset, non_beat_preset, u_time, 0.1);
                    beat_start_time = -1.0;
                }
            }
        } // */
        if backspace_pressed {
            println!("Taking screenshot...");
            screenshot_taker.take_screenshot(&display);
        }

        for image_data in screenshot_taker.pickup_screenshots() {
            let image_name = format!(
                "slime_mould-{}.png",
                Local::now().format("%Y-%m-%d_%H%M%S%.f")
            );
            thread::spawn(move || {
                screenshot::save_screenshot(image_data, image_name);
            });
        }

        if toggle_fullscreen {
            if fullscreen {
                display.gl_window().window().set_fullscreen(None);
                fullscreen = false;
            } else {
                let monitor = display
                    .gl_window()
                    .window()
                    .available_monitors()
                    .next()
                    .unwrap();
                let fs = Fullscreen::Borderless(Some(monitor));
                display.gl_window().window().set_fullscreen(Some(fs));

                fullscreen = true;
            }
        }

        if stop_event_loop {
            action = Action::Stop;
        }

        action
    });
}

pub enum Action {
    Stop,
    Continue,
}

pub fn start_loop<F>(event_loop: EventLoop<()>, mut callback: F)
where
    F: 'static + FnMut(&Vec<Event<'_, ()>>) -> Action,
{
    let mut events_buffer = Vec::new();
    let mut next_frame_time = Instant::now();
    event_loop.run(move |event, _, control_flow| {
        let run_callback = match event.to_static() {
            Some(Event::NewEvents(cause)) => matches!(
                cause,
                StartCause::ResumeTimeReached { .. } | StartCause::Init
            ),
            Some(event) => {
                events_buffer.push(event);
                false
            }
            None => {
                // Ignore this event.
                false
            }
        };

        let action = if run_callback {
            let action = callback(&events_buffer);
            next_frame_time = Instant::now() + Duration::from_nanos(16666667) / 2;
            // TODO: Add back the old accumulator loop in some way

            events_buffer.clear();
            action
        } else {
            Action::Continue
        };

        match action {
            Action::Continue => {
                *control_flow = ControlFlow::WaitUntil(next_frame_time);
            }
            Action::Stop => {
                *control_flow = ControlFlow::Exit;
            }
        }
    });
}

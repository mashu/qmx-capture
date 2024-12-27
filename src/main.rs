use anyhow::Result;
use cpal::traits::*;
use rustfft::{FftPlanner, num_complex::Complex};
use textplots::{Chart, Plot, Shape};
use crossterm::{
    execute, queue,
    terminal::{Clear, ClearType, size, enable_raw_mode, disable_raw_mode, 
               EnterAlternateScreen, LeaveAlternateScreen},
    cursor::{MoveTo, Hide, Show},
    style::{Color, SetForegroundColor, SetBackgroundColor, ResetColor},
    event::{self, Event, KeyCode},
};
use std::{
    sync::{Arc, Mutex},
    io::{stdout, Write, Stdout, stdin},
    fmt::Write as _,
    time::{Duration, Instant},
    thread,
};

const FFT_SIZE: usize = 2048;
const TARGET_FPS: u64 = 30;
const BASE_GAIN: f32 = 10.0;

#[derive(Clone)]
struct AudioBuffer {
    samples: Vec<f32>,
    position: usize,
}

#[derive(Clone)]
struct ViewState {
    gain: f32,
    freq_zoom: f32,
    waterfall_data: Vec<Vec<(f32, f32)>>,
    current_line: usize,
    history_size: usize,
}

impl ViewState {
    fn new(history_size: usize) -> Self {
        Self {
            gain: 5.0,
            freq_zoom: 1.0,
            waterfall_data: vec![vec![(0.0, 0.0); FFT_SIZE/2]; history_size],
            current_line: 0,
            history_size,
        }
    }

    fn add_spectrum(&mut self, spectrum: Vec<f32>, sample_rate: u32) {
        self.waterfall_data[self.current_line] = spectrum.iter()
            .enumerate()
            .map(|(i, &mag)| {
                let freq = i as f32 * sample_rate as f32 / FFT_SIZE as f32;
                (freq, mag)
            })
            .collect();
        self.current_line = (self.current_line + 1) % self.history_size;
    }
}

struct Renderer {
    stdout: Stdout,
    output_buffer: String,
    term_width: u16,
}

impl Renderer {
    fn new() -> Result<Self> {
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, Hide)?;
        enable_raw_mode()?;
        let (term_width, _) = size()?;
        Ok(Self {
            stdout,
            output_buffer: String::with_capacity((term_width as usize) * 3),
            term_width,
        })
    }

    fn render(&mut self, state: &ViewState, sample_rate: u32) -> Result<()> {
        self.output_buffer.clear();
        write!(self.output_buffer, "Gain: {:.1}x | Freq Zoom: {:.1}x | Press 'q' to quit | FPS: {}\n\n", 
               state.gain, state.freq_zoom, TARGET_FPS)?;

        let max_freq = sample_rate as f32 / state.freq_zoom / 2.0;
        write!(self.output_buffer, "Spectrum Analysis (0 Hz - {:.0} Hz)\n", max_freq)?;
        write!(self.output_buffer, "────────────────────────────────\n")?;

        let spectrum_chart = Chart::new(self.term_width as u32, 5, 0.0, max_freq)
            .lineplot(&Shape::Lines(&state.waterfall_data[state.current_line]))
            .to_string();
        write!(self.output_buffer, "{}", spectrum_chart)?;

        queue!(self.stdout, 
            Clear(ClearType::All),
            MoveTo(0, 0)
        )?;
        write!(self.stdout, "{}", self.output_buffer)?;

        for i in 0..state.history_size {
            let line = (state.current_line + i) % state.history_size;
            let points = &state.waterfall_data[line];
            queue!(self.stdout, MoveTo(0, i as u16 + 15))?;

            let freq_step = (sample_rate as f32) / 2.0 / state.freq_zoom / (self.term_width as f32);
            for j in 0..self.term_width as usize {
                let idx = ((j as f32 * freq_step) * FFT_SIZE as f32 / sample_rate as f32) as usize;
                if idx < points.len() {
                    let magnitude = points[idx].1;
                    let normalized = (magnitude * 200.0).min(100.0) as u8;
                    let color = match normalized {
                        0..=20 => Color::Blue,
                        21..=40 => Color::Cyan,
                        41..=60 => Color::Green,
                        61..=80 => Color::Yellow,
                        _ => Color::Red,
                    };
                    queue!(
                        self.stdout,
                        SetBackgroundColor(color),
                        SetForegroundColor(color),
                    )?;
                    write!(self.stdout, "█")?;
                    queue!(self.stdout, ResetColor)?;
                }
            }
            writeln!(self.stdout)?;
        }
        self.stdout.flush()?;
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = execute!(self.stdout, Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn list_devices() -> Result<Vec<cpal::Device>> {
    let host = cpal::default_host();
    let devices = host.input_devices()?;
    println!("Available input devices:\n----------------------");
    
    let device_list: Vec<_> = devices.collect();
    for (idx, device) in device_list.iter().enumerate() {
        if let Ok(name) = device.name() {
            if let Ok(config) = device.default_input_config() {
                println!("{}. {} ({} Hz)", idx, name, config.sample_rate().0);
            } else {
                println!("{}. {}", idx, name);
            }
        }
    }
    Ok(device_list)
}

fn get_user_device_choice(max: usize) -> usize {
    loop {
        println!("\nSelect device number (0-{}): ", max - 1);
        let mut input = String::new();
        stdin().read_line(&mut input).unwrap();
        if let Ok(num) = input.trim().parse() {
            if num < max {
                return num;
            }
        }
        println!("Invalid selection, try again");
    }
}

fn main() -> Result<()> {
    let device_list = list_devices()?;
    if device_list.is_empty() {
        println!("No input devices found!");
        return Ok(());
    }
    
    let device_idx = get_user_device_choice(device_list.len());
    let device = &device_list[device_idx];
    
    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    println!("\nSelected device: {} @ {} Hz", device.name()?, sample_rate);
    println!("Press Enter to start visualization...");
    let mut input = String::new();
    stdin().read_line(&mut input)?;
    
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    
    let (_, term_height) = size()?;
    let history_size = (term_height - 15) as usize;
    
    let mut state = ViewState::new(history_size);
    let buffer = Arc::new(Mutex::new(AudioBuffer {
        samples: vec![0.0; FFT_SIZE],
        position: 0,
    }));
    
    let buffer_clone = Arc::clone(&buffer);
    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mut buffer = buffer_clone.lock().unwrap();
            for &sample in data {
                let pos = buffer.position;
                buffer.samples[pos] = sample;
                buffer.position = (pos + 1) % FFT_SIZE;
            }
        },
        |err| eprintln!("Error in stream: {}", err),
        None,
    )?;
    
    stream.play()?;
    let mut renderer = Renderer::new()?;

    let frame_time = Duration::from_micros(1_000_000 / TARGET_FPS);
    loop {
        let frame_start = Instant::now();

        if event::poll(Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('+') => state.gain *= 1.2,
                    KeyCode::Char('-') => state.gain /= 1.2,
                    KeyCode::Char('w') => state.freq_zoom *= 1.2,
                    KeyCode::Char('s') => state.freq_zoom /= 1.2,
                    _ => (),
                }
            }
        }

        let spectrum = {
            let buffer = buffer.lock().unwrap();
            let mut ordered_samples = vec![0.0; FFT_SIZE];
            let pos = buffer.position;
            
            for i in 0..FFT_SIZE {
                let sample_pos = (pos + FFT_SIZE - i) % FFT_SIZE;
                ordered_samples[FFT_SIZE - 1 - i] = buffer.samples[sample_pos];
            }
            
            let mut fft_buffer: Vec<Complex<f32>> = ordered_samples.iter()
                .enumerate()
                .map(|(i, &sample)| {
                    let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos());
                    Complex::new(sample * window * state.gain * BASE_GAIN, 0.0)
                })
                .collect();
            
            fft.process(&mut fft_buffer);
            
            fft_buffer.iter()
                .take(FFT_SIZE/2)
                .enumerate()
                .map(|(i, x)| {
                    if i == 0 { return 0.0; }
                    let freq_scale = (1.0 + (i as f32 / 100.0)).log10();
                    (x.norm_sqr() as f32).sqrt() * freq_scale
                })
                .collect()
        };
        
        state.add_spectrum(spectrum, sample_rate);
        renderer.render(&state, sample_rate)?;

        let elapsed = frame_start.elapsed();
        if elapsed < frame_time {
            thread::sleep(frame_time - elapsed);
        }
    }
    
    Ok(())
}

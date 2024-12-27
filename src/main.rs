use anyhow::Result;
use cpal::traits::*;
use rustfft::{FftPlanner, num_complex::Complex};
use textplots::{Chart, Plot, Shape};
use crossterm::{
    execute, queue,
    terminal::{size, enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    cursor::{MoveTo, Hide, Show},
    style::{Color, SetForegroundColor, SetBackgroundColor, ResetColor},
    event::{self, Event, KeyCode},
};
use std::{
    sync::{Arc, Mutex},
    io::{stdout, Write, Stdout, stdin},
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

#[derive(Clone, PartialEq)]
struct ScreenCell {
    char: char,
    fg_color: Option<Color>,
    bg_color: Option<Color>,
}

impl Default for ScreenCell {
    fn default() -> Self {
        Self {
            char: ' ',
            fg_color: None,
            bg_color: None,
        }
    }
}

struct ScreenBuffer {
    cells: Vec<Vec<ScreenCell>>,
    width: usize,
    height: usize,
}

impl ScreenBuffer {
    fn new(width: usize, height: usize) -> Self {
        Self {
            cells: vec![vec![ScreenCell::default(); width]; height],
            width,
            height,
        }
    }

    fn clear(&mut self) {
        for row in &mut self.cells {
            for cell in row {
                *cell = ScreenCell::default();
            }
        }
    }
}

struct Renderer {
    stdout: Stdout,
    front_buffer: ScreenBuffer,
    back_buffer: ScreenBuffer,
}

impl Renderer {
    fn new() -> Result<Self> {
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, Hide)?;
        enable_raw_mode()?;
        let (term_width, term_height) = size()?;

        Ok(Self {
            stdout,
            front_buffer: ScreenBuffer::new(term_width as usize, term_height as usize),
            back_buffer: ScreenBuffer::new(term_width as usize, term_height as usize),
        })
    }

    fn write_str_at(&mut self, x: usize, y: usize, s: &str) {
        let cells = &mut self.back_buffer.cells[y];
        for (i, c) in s.chars().enumerate() {
            if x + i >= self.back_buffer.width {
                break;
            }
            cells[x + i].char = c;
        }
    }

    fn set_cell(&mut self, x: usize, y: usize, cell: ScreenCell) {
        if x < self.back_buffer.width && y < self.back_buffer.height {
            self.back_buffer.cells[y][x] = cell;
        }
    }

    fn render(&mut self, state: &ViewState, sample_rate: u32) -> Result<()> {
        self.back_buffer.clear();

        // Render header
        let header = format!("Gain: {:.1}x | Freq Zoom: {:.1}x | Press 'q' to quit | FPS: {}",
                           state.gain, state.freq_zoom, TARGET_FPS);
        self.write_str_at(0, 0, &header);

        let max_freq = sample_rate as f32 / state.freq_zoom / 2.0;
        let spectrum_header = format!("Spectrum Analysis (0 Hz - {:.0} Hz)", max_freq);
        self.write_str_at(0, 2, &spectrum_header);

        self.write_str_at(0, 3, "────────────────────────────────");

        // Render spectrum chart
        let spectrum_chart = Chart::new(self.back_buffer.width as u32, 5, 0.0, max_freq)
            .lineplot(&Shape::Lines(&state.waterfall_data[state.current_line]))
            .to_string();
        for (i, line) in spectrum_chart.lines().enumerate() {
            self.write_str_at(0, 4 + i, line);
        }

        // Render waterfall
        let freq_step = (sample_rate as f32) / 2.0 / state.freq_zoom / (self.back_buffer.width as f32);
        for i in 0..state.history_size {
            let line = (state.current_line + i) % state.history_size;
            let points = &state.waterfall_data[line];

            for j in 0..self.back_buffer.width {
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
                    self.set_cell(j, i + 15, ScreenCell {
                        char: '█',
                        fg_color: Some(color),
                        bg_color: Some(color),
                    });
                }
            }
        }

        // Update screen with changes
        let mut current_fg = None;
        let mut current_bg = None;

        for y in 0..self.back_buffer.height {
            for x in 0..self.back_buffer.width {
                let front_cell = &self.front_buffer.cells[y][x];
                let back_cell = &self.back_buffer.cells[y][x];

                if front_cell != back_cell {
                    queue!(self.stdout, MoveTo(x as u16, y as u16))?;

                    if current_fg != back_cell.fg_color {
                        if let Some(color) = back_cell.fg_color {
                            queue!(self.stdout, SetForegroundColor(color))?;
                        } else {
                            queue!(self.stdout, ResetColor)?;
                        }
                        current_fg = back_cell.fg_color;
                    }

                    if current_bg != back_cell.bg_color {
                        if let Some(color) = back_cell.bg_color {
                            queue!(self.stdout, SetBackgroundColor(color))?;
                        } else {
                            queue!(self.stdout, ResetColor)?;
                        }
                        current_bg = back_cell.bg_color;
                    }

                    write!(self.stdout, "{}", back_cell.char)?;
                }
            }
        }

        self.stdout.flush()?;
        std::mem::swap(&mut self.front_buffer, &mut self.back_buffer);
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
    let input_device = &device_list[device_idx];

    let input_config = input_device.default_input_config()?;
    let sample_rate = input_config.sample_rate().0;
    println!("\nSelected device: {} @ {} Hz", input_device.name()?, sample_rate);
    
    // Select output device
    let host = cpal::default_host();
    let output_device = host.default_output_device()
        .expect("No output device available");
    let output_config = output_device.default_output_config()?;
    
    println!("Press Enter to start visualization...");
    let mut input = String::new();
    stdin().read_line(&mut input)?;

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);

    let (_, term_height) = size()?;
    let history_size = (term_height - 15) as usize;

    let mut state = ViewState::new(history_size);
    
    // Create shared buffers for input and output
    let input_buffer = Arc::new(Mutex::new(AudioBuffer {
        samples: vec![0.0; FFT_SIZE],
        position: 0,
    }));
    let output_buffer = Arc::clone(&input_buffer);

    // Input stream configuration
    let input_buffer_clone = Arc::clone(&input_buffer);
    let input_stream = input_device.build_input_stream(
        &input_config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mut buffer = input_buffer_clone.lock().unwrap();
            for &sample in data {
                let pos = buffer.position;
                buffer.samples[pos] = sample;
                buffer.position = (buffer.position + 1) % FFT_SIZE;
            }
        },
        |err| eprintln!("Error in input stream: {}", err),
        None,
    )?;

    // Output stream configuration
    let output_stream = output_device.build_output_stream(
        &output_config.config(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let mut buffer = output_buffer.lock().unwrap();
            
            // Copy samples to output buffer
            for sample in data.iter_mut() {
                let pos = buffer.position;
                *sample = buffer.samples[pos];
                buffer.position = (buffer.position + 1) % FFT_SIZE;
            }
        },
        |err| eprintln!("Error in output stream: {}", err),
        None,
    )?;

    // Start both streams
    input_stream.play()?;
    output_stream.play()?;

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
            let buffer = input_buffer.lock().unwrap();
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

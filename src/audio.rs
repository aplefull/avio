use ffmpeg_next as ffmpeg;
use ffmpeg::{codec, format, frame, media};
use rodio::{OutputStream, Sink, Source};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ffmpeg_next::{Rational, Rescale};

const MS_TIME_BASE: Rational = Rational(1, 1000);

fn timestamp_to_ms(timestamp: i64, time_base: Rational) -> i64 {
    timestamp.rescale(time_base, MS_TIME_BASE)
}

struct DecodedAudio {
    samples: Vec<f32>,
    sample_rate: u32,
    duration_ms: i64,
}

impl DecodedAudio {
    fn new(filename: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut input = format::input(&filename)?;
        let audio_stream = input
            .streams()
            .best(media::Type::Audio)
            .ok_or("No audio stream found")?;
        let time_base = audio_stream.time_base();
        let context = codec::Context::from_parameters(audio_stream.parameters())?;
        let stream_index = audio_stream.index();

        let mut decoder = context.decoder().audio()?;

        let sample_rate = decoder.rate() / decoder.channels() as u32;
        let channels = decoder.channels();

        println!("Decoding audio: sample rate={}Hz, channels={}", decoder.rate(), channels);
        
        let decoding_start = std::time::Instant::now();
        let mut samples = Vec::new();
        let mut duration_ms = 0;

        for (stream, packet) in input.packets() {
            if stream.index() != stream_index {
                continue;
            }

            if let Some(pts) = packet.pts() {
                let ts_ms = timestamp_to_ms(pts, time_base);
                if ts_ms > duration_ms {
                    duration_ms = ts_ms;
                }
            }

            if let Err(e) = decoder.send_packet(&packet) {
                eprintln!("Error sending packet: {}", e);
                continue;
            }

            let mut decoded = frame::Audio::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                match decoded.format() {
                    format::Sample::F32(format::sample::Type::Planar) => {
                        let frame_samples = decoded.plane::<f32>(0);

                        if channels == 1 {
                            for &sample in frame_samples {
                                samples.push(sample);
                                samples.push(sample);
                            }
                        } else {
                            samples.extend_from_slice(frame_samples);
                        }
                    },
                    other_format => {
                        let mut converted = frame::Audio::empty();
                        if let Ok(_) = ffmpeg::software::resampling::context::Context::get(
                            decoded.format(),
                            decoded.channel_layout(),
                            decoded.rate(),
                            format::Sample::F32(format::sample::Type::Planar),
                            decoded.channel_layout(),
                            decoded.rate(),
                        ).and_then(|mut converter| converter.run(&decoded, &mut converted)) {
                            let frame_samples = converted.plane::<f32>(0);
                            
                            if channels == 1 {
                                for &sample in frame_samples {
                                    samples.push(sample);
                                    samples.push(sample);
                                }
                            } else {
                                samples.extend_from_slice(frame_samples);
                            }
                        } else {
                            println!("Failed to convert audio format {:?}", other_format);
                        }
                    }
                }
            }
        }

        println!("Finished decoding {} audio samples, duration: {}ms, took {}ms",
                 samples.len(), duration_ms, decoding_start.elapsed().as_millis());

        Ok(DecodedAudio {
            samples,
            sample_rate,
            duration_ms,
        })
    }

    fn ms_to_sample_pos(&self, ms: i64) -> usize {
        let samples_per_ms = self.sample_rate as f64 / 1000.0;
        let sample_pos = (ms as f64 * samples_per_ms) as usize;
        sample_pos * 2
    }

    fn sample_pos_to_ms(&self, pos: usize) -> i64 {
        let sample_idx = pos / 2;
        let ms_per_sample = 1000.0 / self.sample_rate as f64;
        (sample_idx as f64 * ms_per_sample) as i64
    }
}

struct MemoryAudioSource {
    decoded_audio: Arc<DecodedAudio>,
    position: usize,
    current_time_ms: Arc<Mutex<i64>>,
}

impl MemoryAudioSource {
    fn new(decoded_audio: Arc<DecodedAudio>, start_pos: usize, current_time_ms: Arc<Mutex<i64>>) -> Self {
        let ms = decoded_audio.sample_pos_to_ms(start_pos);
        *current_time_ms.lock().unwrap() = ms;

        Self {
            decoded_audio,
            position: start_pos,
            current_time_ms,
        }
    }
}

impl Iterator for MemoryAudioSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.position < self.decoded_audio.samples.len() {
            let sample = self.decoded_audio.samples[self.position];

            if self.position % 4000 == 0 {
                let ms = self.decoded_audio.sample_pos_to_ms(self.position);
                *self.current_time_ms.lock().unwrap() = ms;
            }

            self.position += 1;
            Some(sample)
        } else {
            None
        }
    }
}

impl Source for MemoryAudioSource {
    fn channels(&self) -> u16 { 2 }
    fn sample_rate(&self) -> u32 { self.decoded_audio.sample_rate }
    fn current_frame_len(&self) -> Option<usize> { None }
    fn total_duration(&self) -> Option<Duration> {
        let total_seconds = (self.decoded_audio.duration_ms / 1000) as u64;
        Some(Duration::from_secs(total_seconds))
    }
}

impl Clone for MemoryAudioSource {
    fn clone(&self) -> Self {
        Self {
            decoded_audio: self.decoded_audio.clone(),
            position: self.position,
            current_time_ms: self.current_time_ms.clone(),
        }
    }
}

pub struct Audio {
    pub current_time_ms: Arc<Mutex<i64>>,
    decoded_audio: Arc<DecodedAudio>,
    sink: Sink,
    _stream: OutputStream,
    was_playing: Arc<Mutex<bool>>,
}

impl Audio {
    pub fn new(filename: &str) -> Result<Self, Box<dyn std::error::Error>> {
        println!("Loading audio file: {}", filename);

        let decoded_audio = Arc::new(DecodedAudio::new(filename)?);

        let (stream, stream_handle) = OutputStream::try_default()?;
        let sink = Sink::try_new(&stream_handle)?;

        let current_time_ms = Arc::new(Mutex::new(0i64));
        let was_playing = Arc::new(Mutex::new(true));

        let source = MemoryAudioSource::new(
            decoded_audio.clone(),
            0,
            current_time_ms.clone()
        );

        let source = source.repeat_infinite();

        sink.append(source);
        sink.set_volume(0.1);
        sink.play();

        Ok(Audio {
            current_time_ms,
            decoded_audio,
            sink,
            _stream: stream,
            was_playing,
        })
    }

    pub fn seek(&self, target_ms: i64) {
        let was_playing = !self.sink.is_paused();
        *self.was_playing.lock().unwrap() = was_playing;

        let target_ms = target_ms.max(0).min(self.decoded_audio.duration_ms);

        let sample_pos = self.decoded_audio.ms_to_sample_pos(target_ms);

        *self.current_time_ms.lock().unwrap() = target_ms;

        self.sink.stop();
        self.sink.clear();

        let source = MemoryAudioSource::new(
            self.decoded_audio.clone(),
            sample_pos,
            self.current_time_ms.clone()
        );

        let source = source.repeat_infinite();

        self.sink.append(source);

        if was_playing {
            self.sink.play();
        } else {
            self.sink.pause();
        }
    }

    pub fn get_current_time(&self) -> i64 {
        *self.current_time_ms.lock().unwrap()
    }

    pub fn pause(&self) {
        *self.was_playing.lock().unwrap() = false;
        self.sink.pause();
    }

    pub fn play(&self) {
        *self.was_playing.lock().unwrap() = true;
        self.sink.play();
    }

    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.sink.stop();
    }
}
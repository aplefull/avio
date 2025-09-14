extern crate ffmpeg_next as ffmpeg;

use ffmpeg::ffi::AV_TIME_BASE;
use ffmpeg::{
    codec, format, frame, media, rescale,
    software::scaling::{context::Context as ScalingContext, flag::Flags},
    util::format::pixel::Pixel,
    Rational, Rescale,
};
use ffmpeg_next::threading::Type::Frame;

const AV_TIME_BASE_RATIONAL: Rational = Rational(1, AV_TIME_BASE);
const MS_TIME_BASE: Rational = Rational(1, 1000);

fn timestamp_to_ms(timestamp: i64, time_base: Rational) -> i64 {
    timestamp.rescale(time_base, MS_TIME_BASE)
}

fn ms_to_timestamp(ms: i64, time_base: Rational) -> i64 {
    ms.rescale(MS_TIME_BASE, time_base)
}

pub struct VideoFrame {
    pub width: usize,
    pub height: usize,
    pub buffer: Vec<u8>,
}

pub struct Video {
    input_context: format::context::Input,
    decoder: ffmpeg::decoder::Video,
    scaler: ScalingContext,
    stream_index: usize,
    duration_ms: i64,
    framerate: f64,
    current_timestamp_ms: i64,
    time_base: Rational,
    video_width: usize,
    video_height: usize,
    just_seeked: bool,
    seek_target_ms: i64,
    frames_decoded_since_seek: u32,
}

impl Video {
    pub fn new(filename: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut input_context = format::input(&filename)?;
        let video_stream = input_context
            .streams()
            .best(media::Type::Video)
            .ok_or("Could not find video stream")?;
        let stream_index = video_stream.index();
        let time_base = video_stream.time_base();
        let mut decoder_ctx = codec::context::Context::from_parameters(video_stream.parameters())?;

        decoder_ctx.set_threading(
            ffmpeg::threading::Config {
                count: num_cpus::get(),
                kind: Frame,
            }
        );

        let decoder = decoder_ctx.decoder().video()?;

        let reported_duration = timestamp_to_ms(input_context.duration(), AV_TIME_BASE_RATIONAL);

        let fps = Video::get_framerate(&input_context, stream_index);
        let min_reasonable_duration = (1000.0 / fps) as i64 * 10;

        let duration_ms = if reported_duration < min_reasonable_duration {
            println!("Reported duration too small ({}ms) → Calculating from packets...", reported_duration);
            Video::calculate_duration(&mut input_context, stream_index)
        } else {
            reported_duration
        };

        let video_width = decoder.width() as usize;
        let video_height = decoder.height() as usize;

        let scaler = ScalingContext::get(
            decoder.format(),
            video_width as u32,
            video_height as u32,
            Pixel::RGB24,
            video_width as u32,
            video_height as u32,
            Flags::BILINEAR,
        )?;

        Ok(Video {
            input_context,
            decoder,
            scaler,
            stream_index,
            duration_ms,
            framerate: fps,
            current_timestamp_ms: 0,
            time_base,
            video_width,
            video_height,
            just_seeked: false,
            seek_target_ms: 0,
            frames_decoded_since_seek: 0,
        })
    }

    pub fn get_current_timestamp_ms(&self) -> i64 {
        self.current_timestamp_ms
    }

    pub fn get_duration_ms(&self) -> i64 {
        self.duration_ms
    }

    pub fn get_frame_rate(&self) -> f64 {
        self.framerate
    }

    pub fn seek(&mut self, target_ms: i64) -> Result<(), Box<dyn std::error::Error>> {
        self.seek_to_ms_accurate(target_ms)
    }

    pub fn next_frame(&mut self) -> Option<Result<VideoFrame, Box<dyn std::error::Error>>> {
        loop {
            let mut decoded = frame::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(_) => {
                    if let Some(pts) = decoded.pts() {
                        let pts_ms = timestamp_to_ms(pts, self.time_base);
                        
                        if self.just_seeked {
                            self.frames_decoded_since_seek += 1;
                            
                            if self.frames_decoded_since_seek > 300 {
                                self.current_timestamp_ms = pts_ms;
                                self.just_seeked = false;
                                return Some(self.convert_frame(decoded));
                            }
                            
                            if pts_ms == 0 {
                            } else if pts_ms >= self.seek_target_ms {
                                self.current_timestamp_ms = pts_ms;
                                self.just_seeked = false;
                                return Some(self.convert_frame(decoded));
                            } else {
                                self.current_timestamp_ms = pts_ms;
                            }
                        } else {
                            self.current_timestamp_ms = pts_ms;
                            return Some(self.convert_frame(decoded));
                        }
                    } else {
                        if !self.just_seeked {
                            return Some(self.convert_frame(decoded));
                        }
                    }
                }
                Err(_) => match self.input_context.packets().next() {
                    Some((stream, packet)) => {
                        if stream.index() == self.stream_index {
                            if let Err(e) = self.decoder.send_packet(&packet) {
                                return Some(Err(Box::new(e)));
                            }
                        }
                    }
                    None => return None,
                },
            }
        }
    }

    fn calculate_duration(input_context: &mut format::context::Input, stream_index: usize) -> i64 {
        let mut last_pts = 0;
        let time_base = input_context
            .streams()
            .nth(stream_index)
            .map(|s| s.time_base())
            .unwrap_or(Rational(1, AV_TIME_BASE));

        for (_, packet) in input_context.packets() {
            if packet.stream() == stream_index {
                if let Some(pts) = packet.pts() {
                    last_pts = pts.rescale(time_base, MS_TIME_BASE);
                }
            }
        }

        input_context.seek(0, ..0).unwrap();

        last_pts
    }

    fn get_framerate(input_context: &format::context::Input, stream_index: usize) -> f64 {
        let stream = input_context.streams().nth(stream_index);

        match stream {
            Some(s) => {
                let avg_frame_rate = s.avg_frame_rate();

                if avg_frame_rate.denominator() != 0 {
                    return avg_frame_rate.numerator() as f64 / avg_frame_rate.denominator() as f64;
                }

                30.0
            }
            None => {
                30.0
            }
        }
    }

    #[inline]
    fn convert_frame(
        &mut self,
        decoded: frame::Video,
    ) -> Result<VideoFrame, Box<dyn std::error::Error>> {
        let mut rgb_frame = frame::Video::empty();
        self.scaler.run(&decoded, &mut rgb_frame)?;

        let mut buffer = vec![0u8; self.video_width * self.video_height * 4];
        let data = rgb_frame.data(0);
        let line_size = rgb_frame.stride(0);
        
        self.convert_rgb_to_rgba_fast(data, line_size, &mut buffer);

        Ok(VideoFrame {
            width: self.video_width,
            height: self.video_height,
            buffer,
        })
    }
    
    #[inline]
    fn convert_rgb_to_rgba_fast(&self, src: &[u8], line_size: usize, dst: &mut [u8]) {
        for y in 0..self.video_height {
            for x in (0..self.video_width).step_by(8) {
                let chunk_size = std::cmp::min(8, self.video_width - x);
                for i in 0..chunk_size {
                    let src_idx = y * line_size + (x + i) * 3;
                    let dst_idx = (y * self.video_width + x + i) * 4;

                    if src_idx + 2 < src.len() && dst_idx + 3 < dst.len() {
                        dst[dst_idx] = src[src_idx];        
                        dst[dst_idx + 1] = src[src_idx + 1];
                        dst[dst_idx + 2] = src[src_idx + 2];
                        dst[dst_idx + 3] = 0xFF;            
                    }
                }
            }
        }
    }
    
    fn seek_to_ms_accurate(&mut self, target_ms: i64) -> Result<(), Box<dyn std::error::Error>> {
        
        self.decoder.flush();
        
        let target_ts = ms_to_timestamp(target_ms, rescale::TIME_BASE);
        
        self.input_context.seek(target_ts, ..target_ts)?;
        
        self.just_seeked = true;
        self.seek_target_ms = target_ms;
        self.frames_decoded_since_seek = 0;
        self.current_timestamp_ms = target_ms;
        
        Ok(())
    }
}

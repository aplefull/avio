mod audio;
mod media_info;
mod video;

use eframe::egui;
use std::env;
use std::time::Instant;

struct VideoPlayer {
    video: Option<video::Video>,
    audio: Option<audio::Audio>,
    video_texture: Option<egui::TextureHandle>,
    paused: bool,
    last_frame_time: Instant,
    frame_interval: f64,
    fps_counter: FpsCounter,
    volume: f32,
    is_fullscreen: bool,
    show_media_info: bool,
    media_info: Option<media_info::MediaInfo>,
    current_filename: Option<String>,
}

struct FpsCounter {
    fps: f64,
    frame_count: u32,
    last_update: Instant,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            fps: 0.0,
            frame_count: 0,
            last_update: Instant::now(),
        }
    }

    fn update(&mut self) {
        self.frame_count += 1;
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update).as_secs_f64();
        
        if elapsed >= 1.0 {
            self.fps = self.frame_count as f64 / elapsed;
            self.frame_count = 0;
            self.last_update = now;
        }
    }
}

impl VideoPlayer {
    fn new(filename: Option<&str>) -> Result<Self, Box<dyn std::error::Error>> {
        ffmpeg_next::init()?;

        let (video, audio, frame_interval) = if let Some(filename) = filename {
            let video = video::Video::new(filename)?;
            let frame_interval = 1.0 / video.get_frame_rate();
            let audio = audio::Audio::new(filename).ok();
            (Some(video), audio, frame_interval)
        } else {
            (None, None, 1.0 / 30.0)
        };

        let current_filename = filename.map(|s| s.to_string());
        let media_info = if let Some(filename) = filename {
            media_info::get_media_info(filename)
        } else {
            None
        };

        let player = Self {
            video,
            audio,
            video_texture: None,
            paused: false,
            last_frame_time: Instant::now(),
            frame_interval,
            fps_counter: FpsCounter::new(),
            volume: 0.7,
            is_fullscreen: false,
            show_media_info: false,
            media_info,
            current_filename,
        };

        if let Some(audio) = &player.audio {
            audio.set_volume(player.volume);
        }

        Ok(player)
    }

    fn load_video(&mut self, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
        let video = video::Video::new(filename)?;
        self.frame_interval = 1.0 / video.get_frame_rate();
        self.audio = audio::Audio::new(filename).ok();

        if let Some(audio) = &self.audio {
            audio.set_volume(self.volume);
        }

        self.media_info = media_info::get_media_info(filename);
        self.current_filename = Some(filename.to_string());

        self.video = Some(video);
        self.video_texture = None;
        self.paused = false;
        self.last_frame_time = Instant::now();
        Ok(())
    }

    fn should_process_next_frame(&mut self) -> bool {
        if self.paused {
            return false;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame_time).as_secs_f64();
        
        if elapsed >= self.frame_interval {
            self.last_frame_time = now;
            true
        } else {
            false
        }
    }

    fn update_video_frame(&mut self, ctx: &egui::Context) {
        if self.video.is_some() && self.should_process_next_frame() {
            if let Some(video) = &mut self.video {
                if let Some(Ok(frame)) = video.next_frame() {
                let size = [frame.width, frame.height];
                let pixels: Vec<egui::Color32> = frame.buffer
                    .chunks_exact(4)
                    .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                    .collect();

                let color_image = egui::ColorImage {
                    size,
                    pixels,
                };

                if let Some(texture) = &mut self.video_texture {
                    texture.set(color_image, egui::TextureOptions::LINEAR);
                } else {
                    self.video_texture = Some(ctx.load_texture(
                        "video_frame",
                        color_image,
                        egui::TextureOptions::LINEAR,
                    ));
                }

                    self.fps_counter.update();
                }
            }
        }

        if self.video.is_some() && !self.paused && self.fps_counter.frame_count % 150 == 0 {
            if let Some(audio) = &self.audio {
                if let Some(video) = &self.video {
                    let video_time_ms = video.get_current_timestamp_ms();
                    let audio_time_ms = audio.get_current_time();
                    let sync_diff = (video_time_ms - audio_time_ms).abs();

                    if sync_diff > 200 {
                        audio.seek(video_time_ms);
                    }
                }
            }
        }
    }

    fn format_time(ms: i64) -> String {
        let total_seconds = ms / 1000;
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }

    fn format_bitrate(bitrate: Option<usize>) -> String {
        match bitrate {
            Some(br) if br >= 1_000_000 => format!("{:.1} Mbps", br as f64 / 1_000_000.0),
            Some(br) if br >= 1_000 => format!("{:.1} kbps", br as f64 / 1_000.0),
            Some(br) => format!("{} bps", br),
            None => "Unknown".to_string(),
        }
    }

    fn format_duration(ms: i64) -> String {
        if ms > 0 {
            format!("{} ({})", Self::format_time(ms), ms)
        } else {
            "Unknown".to_string()
        }
    }

    fn format_optional_u32(value: Option<u32>) -> String {
        value.map(|v| v.to_string()).unwrap_or_else(|| "Unknown".to_string())
    }

    fn format_optional_u16(value: Option<u16>) -> String {
        value.map(|v| v.to_string()).unwrap_or_else(|| "Unknown".to_string())
    }
}

impl eframe::App for VideoPlayer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.video.is_some() {
            self.update_video_frame(ctx);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let available_size = ui.available_size();

            if self.video.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.0);

                        ui.add(egui::Label::new(
                            egui::RichText::new("Avio Player")
                                .size(32.0)
                                .color(egui::Color32::WHITE)
                        ));

                        ui.add_space(20.0);

                        ui.add(egui::Label::new(
                            egui::RichText::new("Select a video file to start playing")
                                .size(16.0)
                                .color(egui::Color32::LIGHT_GRAY)
                        ));

                        ui.add_space(30.0);

                        if ui.add(egui::Button::new("Open Video File")
                            .min_size(egui::vec2(150.0, 40.0))).clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Video files", &["mp4", "avi", "mkv", "mov", "wmv", "flv", "webm", "m4v"])
                                .add_filter("All files", &["*"])
                                .pick_file()
                            {
                                if let Some(path_str) = path.to_str() {
                                    if let Err(e) = self.load_video(path_str) {
                                        eprintln!("Error loading video: {}", e);
                                    }
                                }
                            }
                        }
                    });
                });
                return;
            }

            let control_height = if self.is_fullscreen { 0.0 } else { 80.0 };
            let video_area_height = available_size.y - control_height;
            let video_area = egui::Rect::from_min_size(
                ui.min_rect().min,
                egui::vec2(available_size.x, video_area_height),
            );

            if let Some(texture) = &self.video_texture {
                let texture_size = texture.size_vec2();
                let aspect_ratio = texture_size.x / texture_size.y;
                
                let display_size = if video_area.width() / video_area.height() > aspect_ratio {
                    egui::vec2(video_area.height() * aspect_ratio, video_area.height())
                } else {
                    egui::vec2(video_area.width(), video_area.width() / aspect_ratio)
                };

                let video_pos = video_area.center() - display_size * 0.5;
                let video_rect = egui::Rect::from_min_size(video_pos, display_size);

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(video_rect), |ui| {
                    ui.add(egui::Image::from_texture(texture).fit_to_exact_size(display_size));
                });
            }

            if !self.is_fullscreen {
                let control_area = egui::Rect::from_min_size(
                    egui::pos2(0.0, video_area_height),
                    egui::vec2(available_size.x, control_height),
                );

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(control_area), |ui| {
                    ui.painter().rect_filled(
                        ui.max_rect(),
                        egui::Rounding::ZERO,
                        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
                    );

                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        ui.add_space(16.0);

                        let current_time = if let Some(video) = &self.video {
                            Self::format_time(video.get_current_timestamp_ms())
                        } else {
                            "00:00:00".to_string()
                        };
                        let total_time = if let Some(video) = &self.video {
                            Self::format_time(video.get_duration_ms())
                        } else {
                            "00:00:00".to_string()
                        };
                        ui.add(egui::Label::new(
                            egui::RichText::new(format!("{} / {}", current_time, total_time))
                                .color(egui::Color32::WHITE)
                                .size(14.0)
                        ));

                        ui.add_space(12.0);

                        let progress = if let Some(video) = &self.video {
                            video.get_current_timestamp_ms() as f32 / video.get_duration_ms() as f32
                        } else {
                            0.0
                        };
                        let available_width = ui.available_width() - 32.0;

                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(available_width, 8.0),
                            egui::Sense::click_and_drag()
                        );

                        ui.painter().rect_filled(
                            rect,
                            egui::Rounding::same(4.0),
                            egui::Color32::from_gray(60),
                        );

                        let fill_width = rect.width() * progress;
                        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_width, rect.height()));
                        ui.painter().rect_filled(
                            fill_rect,
                            egui::Rounding::same(4.0),
                            egui::Color32::from_rgb(100, 150, 255),
                        );

                        if response.hovered() {
                            if let Some(hover_pos) = response.hover_pos() {
                                let hover_x = hover_pos.x.clamp(rect.left(), rect.right());
                                ui.painter().circle_filled(
                                    egui::pos2(hover_x, rect.center().y),
                                    6.0,
                                    egui::Color32::WHITE,
                                );
                            }
                        }

                        if (response.clicked() || response.dragged()) && self.video.is_some() {
                            if let Some(pointer_pos) = response.interact_pointer_pos() {
                                let relative_pos = (pointer_pos.x - rect.left()) / rect.width();
                                let seek_progress = relative_pos.clamp(0.0, 1.0);

                                if let Some(video) = &mut self.video {
                                    let target_ms = (video.get_duration_ms() as f32 * seek_progress) as i64;

                                    if let Err(e) = video.seek(target_ms) {
                                        eprintln!("Seek error: {}", e);
                                    }

                                    if let Some(audio) = &self.audio {
                                        audio.seek(target_ms);
                                    }
                                }
                            }
                        }

                        ui.add_space(16.0);
                    });

                    ui.add_space(16.0);

                    ui.horizontal(|ui| {
                        ui.add_space(16.0);

                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let button_text = if self.paused { "▶" } else { "⏸" };
                            let play_button = egui::Button::new(
                                egui::RichText::new(button_text).size(16.0).color(egui::Color32::WHITE)
                            )
                            .min_size(egui::vec2(40.0, 32.0))
                            .fill(egui::Color32::from_gray(40));

                            if ui.add(play_button).clicked() {
                                self.paused = !self.paused;
                                if let Some(audio) = &self.audio {
                                    if self.paused {
                                        audio.pause();
                                    } else {
                                        audio.play();
                                    }
                                }
                            }

                            ui.add_space(8.0);

                            let back_button = egui::Button::new(
                                egui::RichText::new("⏪").size(14.0).color(egui::Color32::WHITE)
                            )
                            .min_size(egui::vec2(36.0, 32.0))
                            .fill(egui::Color32::from_gray(40));

                            if ui.add(back_button).clicked() && self.video.is_some() {
                                if let Some(video) = &mut self.video {
                                    let target_ms = (video.get_current_timestamp_ms() - 10000).max(0);
                                    if let Err(e) = video.seek(target_ms) {
                                        eprintln!("Seek error: {}", e);
                                    }
                                    if let Some(audio) = &self.audio {
                                        audio.seek(target_ms);
                                    }
                                }
                            }

                            ui.add_space(12.0);

                            let open_button = egui::Button::new(
                                egui::RichText::new("📁").size(14.0).color(egui::Color32::WHITE)
                            )
                            .min_size(egui::vec2(36.0, 32.0))
                            .fill(egui::Color32::from_gray(40));

                            if ui.add(open_button).clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Video files", &["mp4", "avi", "mkv", "mov", "wmv", "flv", "webm", "m4v"])
                                    .add_filter("All files", &["*"])
                                    .pick_file()
                                {
                                    if let Some(path_str) = path.to_str() {
                                        if let Err(e) = self.load_video(path_str) {
                                            eprintln!("Error loading video: {}", e);
                                        }
                                    }
                                }
                            }

                            ui.add_space(8.0);

                            let info_button = egui::Button::new(
                                egui::RichText::new("ℹ").size(14.0).color(egui::Color32::WHITE)
                            )
                            .min_size(egui::vec2(36.0, 32.0))
                            .fill(egui::Color32::from_gray(40));

                            if ui.add(info_button).clicked() {
                                self.show_media_info = !self.show_media_info;
                            }

                            ui.add_space(8.0);

                            let forward_button = egui::Button::new(
                                egui::RichText::new("⏩").size(14.0).color(egui::Color32::WHITE)
                            )
                            .min_size(egui::vec2(36.0, 32.0))
                            .fill(egui::Color32::from_gray(40));

                            if ui.add(forward_button).clicked() && self.video.is_some() {
                                if let Some(video) = &mut self.video {
                                    let target_ms = (video.get_current_timestamp_ms() + 10000)
                                        .min(video.get_duration_ms());
                                    if let Err(e) = video.seek(target_ms) {
                                        eprintln!("Seek error: {}", e);
                                    }
                                    if let Some(audio) = &self.audio {
                                        audio.seek(target_ms);
                                    }
                                }
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(16.0);

                            let fullscreen_icon = if self.is_fullscreen { "⛶" } else { "⛶" };
                            let fullscreen_button = egui::Button::new(
                                egui::RichText::new(fullscreen_icon).size(14.0).color(egui::Color32::WHITE)
                            )
                            .min_size(egui::vec2(36.0, 32.0))
                            .fill(egui::Color32::from_gray(40));

                            if ui.add(fullscreen_button).clicked() {
                                self.is_fullscreen = !self.is_fullscreen;
                            }

                            ui.add_space(12.0);

                            ui.add(egui::Label::new(
                                egui::RichText::new("🔊").size(14.0).color(egui::Color32::WHITE)
                            ));
                            ui.add_space(4.0);
                            let volume_response = ui.add_sized(
                                [80.0, 20.0],
                                egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false)
                            );

                            if volume_response.changed() {
                                if let Some(audio) = &self.audio {
                                    audio.set_volume(self.volume);
                                }
                            }

                            ui.add_space(20.0);

                            ui.add(egui::Label::new(
                                egui::RichText::new(format!("FPS: {:.1}", self.fps_counter.fps))
                                    .size(12.0)
                                    .color(egui::Color32::from_gray(180))
                            ));
                        });
                    });

                    ui.add_space(12.0);
                    });
                });
            }
        });

        if self.show_media_info {
            egui::Window::new("Media Information")
                .default_size([600.0, 400.0])
                .resizable(true)
                .show(ctx, |ui| {
                    if let Some(media_info) = &self.media_info {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.heading("File Information");
                            ui.separator();

                            if let Some(filename) = &self.current_filename {
                                ui.horizontal(|ui| {
                                    ui.label("Path:");
                                    ui.label(filename);
                                });
                            }

                            ui.horizontal(|ui| {
                                ui.label("Format:");
                                ui.label(format!("{} ({})", media_info.format_name, media_info.format_description));
                            });

                            ui.horizontal(|ui| {
                                ui.label("Duration:");
                                ui.label(Self::format_duration(media_info.duration_ms));
                            });

                            ui.horizontal(|ui| {
                                ui.label("Overall Bitrate:");
                                ui.label(Self::format_bitrate(media_info.bit_rate));
                            });

                            ui.add_space(15.0);

                            if !media_info.video_streams.is_empty() {
                                ui.heading("Video Streams");
                                ui.separator();

                                for (i, stream) in media_info.video_streams.iter().enumerate() {
                                    ui.label(format!("Stream {} (Index: {})", i, stream.index));
                                    ui.horizontal(|ui| {
                                        ui.label("  Resolution:");
                                        ui.label(format!("{}x{}", Self::format_optional_u32(stream.width), Self::format_optional_u32(stream.height)));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Codec:");
                                        ui.label(format!("{} ({})", stream.codec_name, stream.codec_description));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Codec ID:");
                                        ui.label(&stream.codec_id);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Frame Rate:");
                                        let fps = stream.frame_rate
                                            .as_ref()
                                            .map(|fr| format!("{:.3} fps ({}/{})", fr.value, fr.numerator, fr.denominator))
                                            .unwrap_or_else(|| "Unknown".to_string());
                                        ui.label(fps);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Pixel Format:");
                                        let pixel_fmt = stream.pixel_format
                                            .as_ref()
                                            .map(|pf| format!("{:?}", pf))
                                            .unwrap_or_else(|| "Unknown".to_string());
                                        ui.label(pixel_fmt);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Bitrate:");
                                        ui.label(Self::format_bitrate(stream.bit_rate));
                                    });
                                    if let Some(frames) = stream.frames {
                                        ui.horizontal(|ui| {
                                            ui.label("  Frame Count:");
                                            ui.label(frames.to_string());
                                        });
                                    }
                                    if let Some(ref aspect_ratio) = stream.aspect_ratio {
                                        ui.horizontal(|ui| {
                                            ui.label("  Aspect Ratio:");
                                            ui.label(format!("{:.3} ({}/{})", aspect_ratio.value, aspect_ratio.numerator, aspect_ratio.denominator));
                                        });
                                    }
                                    if let Some(ref color_space) = stream.color_space {
                                        ui.horizontal(|ui| {
                                            ui.label("  Color Space:");
                                            ui.label(format!("{:?}", color_space));
                                        });
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label("  Time Base:");
                                        ui.label(format!("{}/{} ({:.6})", stream.time_base.numerator, stream.time_base.denominator, stream.time_base.value));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Disposition:");
                                        ui.label(format!("0x{:X}", stream.disposition));
                                    });
                                    if let Some(ref capabilities) = stream.codec_capabilities {
                                        ui.horizontal(|ui| {
                                            ui.label("  Codec Capabilities:");
                                            ui.label(format!("{:?}", capabilities));
                                        });
                                    }
                                    if let Some(ref profiles) = stream.codec_profiles {
                                        ui.horizontal(|ui| {
                                            ui.label("  Codec Profiles:");
                                            let profile_names: Vec<String> = profiles.iter().map(|p| format!("{:?}", p)).collect();
                                            ui.label(profile_names.join(", "));
                                        });
                                    }
                                    if !stream.metadata.is_empty() {
                                        ui.collapsing("  Video Stream Metadata", |ui| {
                                            for (key, value) in &stream.metadata {
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("    {}:", key));
                                                    ui.label(value);
                                                });
                                            }
                                        });
                                    }
                                    ui.add_space(10.0);
                                }
                                ui.add_space(10.0);
                            }

                            if !media_info.audio_streams.is_empty() {
                                ui.heading("Audio Streams");
                                ui.separator();

                                for (i, stream) in media_info.audio_streams.iter().enumerate() {
                                    ui.label(format!("Stream {} (Index: {})", i, stream.index));
                                    ui.horizontal(|ui| {
                                        ui.label("  Sample Rate:");
                                        ui.label(format!("{} Hz", Self::format_optional_u32(stream.sample_rate)));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Channels:");
                                        ui.label(Self::format_optional_u16(stream.channels));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Codec:");
                                        ui.label(format!("{} ({})", stream.codec_name, stream.codec_description));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Codec ID:");
                                        ui.label(&stream.codec_id);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Sample Format:");
                                        let sample_fmt = stream.sample_format
                                            .as_ref()
                                            .map(|sf| format!("{:?}", sf))
                                            .unwrap_or_else(|| "Unknown".to_string());
                                        ui.label(sample_fmt);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Bitrate:");
                                        ui.label(Self::format_bitrate(stream.bit_rate));
                                    });
                                    if let Some(ref channel_layout) = stream.channel_layout {
                                        ui.horizontal(|ui| {
                                            ui.label("  Channel Layout:");
                                            ui.label(format!("{:?}", channel_layout));
                                        });
                                    }
                                    if let Some(frames) = stream.frames {
                                        ui.horizontal(|ui| {
                                            ui.label("  Frame Count:");
                                            ui.label(frames.to_string());
                                        });
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label("  Time Base:");
                                        ui.label(format!("{}/{} ({:.6})", stream.time_base.numerator, stream.time_base.denominator, stream.time_base.value));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Disposition:");
                                        ui.label(format!("0x{:X}", stream.disposition));
                                    });
                                    if let Some(ref capabilities) = stream.codec_capabilities {
                                        ui.horizontal(|ui| {
                                            ui.label("  Codec Capabilities:");
                                            ui.label(format!("{:?}", capabilities));
                                        });
                                    }
                                    if let Some(ref profiles) = stream.codec_profiles {
                                        ui.horizontal(|ui| {
                                            ui.label("  Codec Profiles:");
                                            let profile_names: Vec<String> = profiles.iter().map(|p| format!("{:?}", p)).collect();
                                            ui.label(profile_names.join(", "));
                                        });
                                    }
                                    if let Some(ref profile) = stream.profile {
                                        ui.horizontal(|ui| {
                                            ui.label("  Profile:");
                                            ui.label(format!("{:?}", profile));
                                        });
                                    }
                                    if !stream.metadata.is_empty() {
                                        ui.collapsing("  Audio Stream Metadata", |ui| {
                                            for (key, value) in &stream.metadata {
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("    {}:", key));
                                                    ui.label(value);
                                                });
                                            }
                                        });
                                    }
                                    ui.add_space(10.0);
                                }
                                ui.add_space(10.0);
                            }

                            if !media_info.subtitle_streams.is_empty() {
                                ui.heading("Subtitle Streams");
                                ui.separator();

                                for (i, stream) in media_info.subtitle_streams.iter().enumerate() {
                                    ui.label(format!("Stream {} (Index: {})", i, stream.index));
                                    ui.horizontal(|ui| {
                                        ui.label("  Codec:");
                                        ui.label(&stream.codec_name);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Codec ID:");
                                        ui.label(&stream.codec_id);
                                    });
                                    if let Some(ref language) = stream.language {
                                        ui.horizontal(|ui| {
                                            ui.label("  Language:");
                                            ui.label(language);
                                        });
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label("  Time Base:");
                                        ui.label(format!("{}/{} ({:.6})", stream.time_base.numerator, stream.time_base.denominator, stream.time_base.value));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Disposition:");
                                        ui.label(format!("0x{:X}", stream.disposition));
                                    });
                                    if !stream.metadata.is_empty() {
                                        ui.collapsing("  Subtitle Stream Metadata", |ui| {
                                            for (key, value) in &stream.metadata {
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("    {}:", key));
                                                    ui.label(value);
                                                });
                                            }
                                        });
                                    }
                                    ui.add_space(10.0);
                                }
                                ui.add_space(10.0);
                            }

                            if !media_info.other_streams.is_empty() {
                                ui.heading("Other Streams");
                                ui.separator();

                                for (i, stream) in media_info.other_streams.iter().enumerate() {
                                    ui.label(format!("Stream {} (Index: {})", i, stream.index));
                                    ui.horizontal(|ui| {
                                        ui.label("  Type:");
                                        ui.label(&stream.stream_type);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Codec:");
                                        ui.label(&stream.codec_name);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Codec ID:");
                                        ui.label(&stream.codec_id);
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Time Base:");
                                        ui.label(format!("{}/{} ({:.6})", stream.time_base.numerator, stream.time_base.denominator, stream.time_base.value));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  Disposition:");
                                        ui.label(format!("0x{:X}", stream.disposition));
                                    });
                                    if !stream.metadata.is_empty() {
                                        ui.collapsing("  Other Stream Metadata", |ui| {
                                            for (key, value) in &stream.metadata {
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("    {}:", key));
                                                    ui.label(value);
                                                });
                                            }
                                        });
                                    }
                                    ui.add_space(10.0);
                                }
                                ui.add_space(10.0);
                            }

                            if !media_info.chapters.is_empty() {
                                ui.heading("Chapters");
                                ui.separator();

                                for chapter in media_info.chapters.iter() {
                                    ui.label(format!("Chapter {}: {}", chapter.index, chapter.title));
                                    ui.horizontal(|ui| {
                                        ui.label("  Start:");
                                        ui.label(Self::format_duration(chapter.start_time_ms));
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("  End:");
                                        ui.label(Self::format_duration(chapter.end_time_ms));
                                    });
                                    if !chapter.metadata.is_empty() {
                                        ui.collapsing("  Chapter Metadata", |ui| {
                                            for (key, value) in &chapter.metadata {
                                                ui.horizontal(|ui| {
                                                    ui.label(format!("    {}:", key));
                                                    ui.label(value);
                                                });
                                            }
                                        });
                                    }
                                    ui.add_space(5.0);
                                }
                                ui.add_space(10.0);
                            }

                            if !media_info.metadata.is_empty() {
                                ui.heading("Global Metadata");
                                ui.separator();

                                for (key, value) in &media_info.metadata {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("{}:", key));
                                        ui.label(value);
                                    });
                                }
                                ui.add_space(10.0);
                            }
                        });
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.0);
                            ui.label("No media information available");
                        });
                    }

                    ui.add_space(15.0);
                    if ui.button("Close").clicked() {
                        self.show_media_info = false;
                    }
                });
        }

        if self.video.is_some() && !self.paused {
            ctx.request_repaint();
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.is_fullscreen {
            self.is_fullscreen = false;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.paused = !self.paused;
            if let Some(audio) = &self.audio {
                if self.paused {
                    audio.pause();
                } else {
                    audio.play();
                }
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) && self.video.is_some() {
            if let Some(video) = &mut self.video {
                let target_ms = (video.get_current_timestamp_ms() - 5000).max(0);
                if let Err(e) = video.seek(target_ms) {
                    eprintln!("Seek error: {}", e);
                }
                if let Some(audio) = &self.audio {
                    audio.seek(target_ms);
                }
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) && self.video.is_some() {
            if let Some(video) = &mut self.video {
                let target_ms = (video.get_current_timestamp_ms() + 5000)
                    .min(video.get_duration_ms());
                if let Err(e) = video.seek(target_ms) {
                    eprintln!("Seek error: {}", e);
                }
                if let Some(audio) = &self.audio {
                    audio.seek(target_ms);
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    let player = if args.len() == 2 {
        VideoPlayer::new(Some(&args[1]))?
    } else {
        VideoPlayer::new(None)?
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("Avio Player"),
        ..Default::default()
    };

    eframe::run_native(
        "Avio Player",
        options,
        Box::new(|_cc| {
            Ok(Box::new(player))
        }),
    )?;

    Ok(())
}
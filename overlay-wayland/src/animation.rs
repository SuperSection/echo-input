use std::time::{Duration, Instant};
use input_core::overlay::OverlayConfig;

const DEFAULT_FADE_DURATION: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationState {
    Idle,
    Visible,
    Fading,
}

pub struct Animation {
    state: AnimationState,
    shown_at: Instant,
    fade_start: Instant,
    display_duration: Duration,
    fade_duration: Duration,
    current_opacity: f32,
    target_opacity: f32,
    dirty: bool,
}

impl Animation {
    pub fn new(config: &OverlayConfig) -> Self {
        Self {
            state: AnimationState::Idle,
            shown_at: Instant::now(),
            fade_start: Instant::now(),
            display_duration: config.display_duration,
            fade_duration: DEFAULT_FADE_DURATION,
            current_opacity: 0.0,
            target_opacity: config.opacity,
            dirty: false,
        }
    }

    pub fn show(&mut self, opacity: f32) {
        self.state = AnimationState::Visible;
        self.shown_at = Instant::now();
        self.current_opacity = opacity;
        self.target_opacity = opacity;
        self.dirty = true;
    }

    pub fn update_config(&mut self, config: &OverlayConfig) {
        self.display_duration = config.display_duration;
        self.target_opacity = config.opacity;
        if self.state == AnimationState::Visible {
            self.current_opacity = config.opacity;
        }
    }

    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;
        match self.state {
            AnimationState::Idle => {}
            AnimationState::Visible => {
                if now.duration_since(self.shown_at) >= self.display_duration {
                    self.state = AnimationState::Fading;
                    self.fade_start = now;
                    changed = true;
                }
            }
            AnimationState::Fading => {
                let elapsed = now.duration_since(self.fade_start);
                if elapsed >= self.fade_duration {
                    self.current_opacity = 0.0;
                    self.state = AnimationState::Idle;
                    changed = true;
                } else {
                    let progress = elapsed.as_secs_f32() / self.fade_duration.as_secs_f32();
                    self.current_opacity = self.target_opacity * (1.0 - progress);
                    changed = true;
                }
            }
        }
        if self.dirty {
            self.dirty = false;
            changed = true;
        }
        changed
    }

    pub fn current_opacity(&self) -> f32 {
        self.current_opacity
    }

    pub fn is_visible(&self) -> bool {
        self.state != AnimationState::Idle
    }

    pub fn state(&self) -> AnimationState {
        self.state
    }

    pub fn time_until_fade(&self) -> Duration {
        match self.state {
            AnimationState::Visible => {
                let elapsed = self.shown_at.elapsed();
                if elapsed >= self.display_duration {
                    Duration::ZERO
                } else {
                    self.display_duration - elapsed
                }
            }
            _ => Duration::ZERO,
        }
    }
}

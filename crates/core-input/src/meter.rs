#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelReading {
    pub rms: f32,
    pub peak: f32,
    pub clipped: bool,
}

impl LevelReading {
    pub const fn silence() -> Self {
        Self {
            rms: 0.0,
            peak: 0.0,
            clipped: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LevelMeter {
    reading: LevelReading,
}

impl LevelMeter {
    pub fn new() -> Self {
        Self {
            reading: LevelReading::silence(),
        }
    }

    pub fn update(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }

        let mut peak = 0.0_f32;
        let mut sum = 0.0_f32;
        let mut clipped = false;

        for &sample in samples {
            let magnitude = sample.abs();
            if magnitude > peak {
                peak = magnitude;
            }
            if magnitude >= 1.0 {
                clipped = true;
            }
            sum += sample * sample;
        }

        let rms = (sum / samples.len() as f32).sqrt();

        self.reading = LevelReading { rms, peak, clipped };
    }

    pub fn reading(&self) -> LevelReading {
        self.reading
    }

    pub fn reset(&mut self) {
        self.reading = LevelReading::silence();
    }
}

impl Default for LevelMeter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{LevelMeter, LevelReading};
    use approx::assert_relative_eq;

    #[test]
    fn meter_reports_silence_for_empty_samples() {
        let mut meter = LevelMeter::new();
        meter.update(&[]);
        assert_eq!(meter.reading(), LevelReading::silence());
    }

    #[test]
    fn meter_computes_peak_and_rms() {
        let mut meter = LevelMeter::new();
        meter.update(&[0.0, 0.5, -0.5]);
        let reading = meter.reading();
        assert_relative_eq!(reading.peak, 0.5, epsilon = 1e-6);
        assert_relative_eq!(reading.rms, (1.0 / 6.0_f32).sqrt(), epsilon = 1e-6);
        assert!(!reading.clipped);
    }

    #[test]
    fn meter_flags_clipping() {
        let mut meter = LevelMeter::new();
        meter.update(&[0.2, -1.2]);
        let reading = meter.reading();
        assert!(reading.clipped);
    }
}

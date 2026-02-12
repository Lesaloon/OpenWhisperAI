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

    pub fn rms_dbfs(&self) -> f32 {
        to_dbfs(self.rms)
    }

    pub fn peak_dbfs(&self) -> f32 {
        to_dbfs(self.peak)
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
        let mut count = 0_u32;

        for &sample in samples {
            if !sample.is_finite() {
                continue;
            }
            let magnitude = sample.abs();
            if magnitude > peak {
                peak = magnitude;
            }
            if magnitude >= 1.0 {
                clipped = true;
            }
            sum += sample * sample;
            count += 1;
        }

        if count == 0 {
            return;
        }

        let rms = (sum / count as f32).sqrt();

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

fn to_dbfs(value: f32) -> f32 {
    if value <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * value.log10()
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

    #[test]
    fn meter_skips_non_finite_samples() {
        let mut meter = LevelMeter::new();
        meter.update(&[f32::NAN, f32::INFINITY, -0.75]);
        let reading = meter.reading();
        assert_relative_eq!(reading.peak, 0.75, epsilon = 1e-6);
        assert_relative_eq!(reading.rms, 0.75, epsilon = 1e-6);
    }

    #[test]
    fn meter_reports_dbfs() {
        let mut meter = LevelMeter::new();
        meter.update(&[1.0]);
        let reading = meter.reading();
        assert_relative_eq!(reading.peak_dbfs(), 0.0, epsilon = 1e-6);
        assert_relative_eq!(reading.rms_dbfs(), 0.0, epsilon = 1e-6);
    }
}

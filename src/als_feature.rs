//! One complete Windows Sensor HID feature report, including its Report ID.
//! Sensor wire enums are one-based (Microsoft DMF HID sensor definitions),
//! even though named-array descriptors use zero-based logical ranges.
pub const REPORT_ID: u8 = 1;
pub const REPORT_LEN: usize = 19;
pub const MINIMUM_INTERVAL_MS: u32 = 100;

pub struct FeatureValues {
    pub reporting_state: u8,
    pub power_state: u8,
    pub report_interval_ms: u32,
    pub change_sensitivity_percent_x100: u16,
}

impl FeatureValues {
    pub const fn new() -> Self {
        Self {
            reporting_state: 2,
            power_state: 2,
            report_interval_ms: 250,
            change_sensitivity_percent_x100: 2500,
        }
    }

    pub fn encode(&self, buffer: &mut [u8]) -> Option<usize> {
        if buffer.len() < REPORT_LEN {
            return None;
        }
        buffer[..REPORT_LEN].fill(0);
        buffer[0] = REPORT_ID;
        buffer[1] = self.reporting_state;
        buffer[2] = self.power_state;
        buffer[3..7].copy_from_slice(&self.report_interval_ms.to_le_bytes());
        buffer[7] = 2; // PC Attached
        // 8..12: sensor status, zero means success.
        buffer[12..14].copy_from_slice(&self.change_sensitivity_percent_x100.to_le_bytes());
        buffer[14..18].copy_from_slice(&MINIMUM_INTERVAL_MS.to_le_bytes());
        buffer[18] = 2; // Ready
        Some(REPORT_LEN)
    }

    pub fn apply(&mut self, data: &[u8]) -> bool {
        if data.len() < REPORT_LEN
            || data[0] != REPORT_ID
            || !(1..=6).contains(&data[1])
            || !(1..=6).contains(&data[2])
        {
            return false;
        }
        let sensitivity = u16::from_le_bytes([data[12], data[13]]);
        if sensitivity > 10_000 {
            return false;
        }
        self.reporting_state = data[1];
        self.power_state = data[2];
        self.report_interval_ms =
            u32::from_le_bytes([data[3], data[4], data[5], data[6]]).max(MINIMUM_INTERVAL_MS);
        self.change_sensitivity_percent_x100 = sensitivity;
        // Ignore host copies of read-only connection/status/minimum/state fields.
        true
    }

    pub fn should_report(&self, changed: bool) -> bool {
        self.power_state == 2
            && match self.reporting_state {
                2 | 5 => true,
                3 | 6 => changed,
                _ => false,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn get_feature_returns_full_wire_report_with_id() {
        let mut report = [0u8; REPORT_LEN];
        assert_eq!(FeatureValues::new().encode(&mut report), Some(19));
        assert_eq!(
            report,
            [
                1, 2, 2, 250, 0, 0, 0, 2, 0, 0, 0, 0, 0xc4, 0x09, 100, 0, 0, 0, 2
            ]
        );
        assert_eq!(FeatureValues::new().encode(&mut [0u8; 18]), None);
    }
    #[test]
    fn set_feature_decodes_id_and_offsets_and_preserves_readonly_fields() {
        let mut report = [0u8; REPORT_LEN];
        let mut v = FeatureValues::new();
        v.encode(&mut report);
        report[1] = 3;
        report[3..7].copy_from_slice(&20u32.to_le_bytes());
        report[7] = 3;
        report[12..14].copy_from_slice(&1000u16.to_le_bytes());
        assert!(v.apply(&report));
        v.encode(&mut report);
        assert_eq!(v.report_interval_ms, 100);
        assert_eq!(v.change_sensitivity_percent_x100, 1000);
        assert_eq!(report[7], 2);
        assert!(!v.apply(&report[..18]));
        report[0] = 2;
        assert!(!v.apply(&report));
    }
    #[test]
    fn reporting_modes_include_windows_threshold_and_wake_modes() {
        let mut v = FeatureValues::new();
        for mode in [2, 5] {
            v.reporting_state = mode;
            assert!(v.should_report(false));
        }
        for mode in [3, 6] {
            v.reporting_state = mode;
            assert!(!v.should_report(false));
            assert!(v.should_report(true));
        }
        for mode in [1, 4] {
            v.reporting_state = mode;
            assert!(!v.should_report(true));
        }
        v.reporting_state = 2;
        v.power_state = 5;
        assert!(!v.should_report(true));
    }
}

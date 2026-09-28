#![no_std]
#![no_main]

use core::cell::RefCell;

mod als_feature;
use als_feature::FeatureValues;

use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::i2c::{self, InterruptHandler as I2cInterruptHandler};
use embassy_rp::peripherals::{I2C0, USB};
use embassy_rp::usb::{self, Driver};
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State as CdcState};
use embassy_usb::class::hid::{
    Config as HidConfig, HidBootProtocol, HidSubclass, HidWriter, ReportId, RequestHandler,
    State as HidState,
};
use embassy_usb::control::OutResponse;
use embassy_usb::{Builder, Config as UsbConfig, UsbDevice};
use panic_halt as _;
use static_cell::StaticCell;

const BH1750_ADDRESS: u8 = 0x23;
const BH1750_POWER_ON: u8 = 0x01;
const BH1750_CONTINUOUS_HIGH_RES_MODE: u8 = 0x10;
const REPORT_ID_INPUT: u8 = 1;

const SENSOR_EVENT_DATA_UPDATED: u8 = 4;
const SENSOR_STATE_READY: u8 = 2;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    I2C0_IRQ => I2cInterruptHandler<I2C0>;
});

struct AlsRequestHandler {
    values: &'static critical_section::Mutex<RefCell<FeatureValues>>,
}

impl RequestHandler for AlsRequestHandler {
    fn get_report(&mut self, id: ReportId, buffer: &mut [u8]) -> Option<usize> {
        if id != ReportId::Feature(als_feature::REPORT_ID) {
            return None;
        }
        // Embassy sends this buffer verbatim: the Report ID is part of the response.
        critical_section::with(|cs| self.values.borrow(cs).borrow().encode(buffer))
    }

    fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
        if id != ReportId::Feature(als_feature::REPORT_ID) {
            return OutResponse::Rejected;
        }
        if critical_section::with(|cs| self.values.borrow(cs).borrow_mut().apply(data)) {
            OutResponse::Accepted
        } else {
            OutResponse::Rejected
        }
    }
}

static ALS_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x20, 0x0A, 0x41, 0x00, 0xA1, 0x00, // Sensor / Ambient Light physical collection
    0x85, 0x01, 0x0A, 0x01, 0x02, 0x15, 0x00, 0x25, 0x06, 0x75, 0x08, 0x95, 0x01, 0xA1, 0x02, 0x0A,
    0x00, 0x08, 0x0A, 0x01, 0x08, 0x0A, 0x02, 0x08, 0x0A, 0x03, 0x08, 0x0A, 0x04, 0x08, 0x0A, 0x05,
    0x08, 0x0A, 0x06, 0x08, 0x81, 0x00, 0xC0, // Sensor State array
    0x0A, 0x02, 0x02, 0x15, 0x00, 0x25, 0x05, 0x75, 0x08, 0x95, 0x01, 0xA1, 0x02, 0x0A, 0x10, 0x08,
    0x0A, 0x11, 0x08, 0x0A, 0x12, 0x08, 0x0A, 0x13, 0x08, 0x0A, 0x14, 0x08, 0x0A, 0x15, 0x08, 0x81,
    0x00, 0xC0, // Sensor Event array
    0x0A, 0xD1, 0x04, 0x15, 0x00, 0x26, 0x10, 0x27, 0x75, 0x10, 0x95, 0x01, 0x81, 0x02, 0x85, 0x01,
    0x0A, 0x16, 0x03, 0x15, 0x00, 0x25, 0x05, 0x75, 0x08, 0x95, 0x01, 0xA1, 0x02, 0x0A, 0x40, 0x08,
    0x0A, 0x41, 0x08, 0x0A, 0x42, 0x08, 0x0A, 0x43, 0x08, 0x0A, 0x44, 0x08, 0x0A, 0x45, 0x08, 0xB1,
    0x00, 0xC0, // Reporting state array
    0x85, 0x01, 0x0A, 0x19, 0x03, 0x15, 0x00, 0x25, 0x05, 0x75, 0x08, 0x95, 0x01, 0xA1, 0x02, 0x0A,
    0x50, 0x08, 0x0A, 0x51, 0x08, 0x0A, 0x52, 0x08, 0x0A, 0x53, 0x08, 0x0A, 0x54, 0x08, 0x0A, 0x55,
    0x08, 0xB1, 0x00, 0xC0, // Power state array
    0x85, 0x01, 0x0A, 0x0E, 0x03, 0x15, 0x00, 0x27, 0xFF, 0xFF, 0xFF, 0xFF, 0x75, 0x20, 0x95, 0x01,
    0x55, 0x00, 0xB1, 0x02, // Report interval (ms)
    0x85, 0x01, 0x0A, 0x09, 0x03, 0x15, 0x00, 0x25, 0x02, 0x75, 0x08, 0x95, 0x01, 0xA1, 0x02, 0x0A,
    0x30, 0x08, 0x0A, 0x31, 0x08, 0x0A, 0x32, 0x08, 0xB1, 0x01,
    0xC0, // Connection type constant array
    0x85, 0x01, 0x0A, 0x03, 0x03, 0x15, 0x00, 0x27, 0xFF, 0xFF, 0xFF, 0xFF, 0x75, 0x20, 0x95, 0x01,
    0xB1, 0x02, // Sensor Status (32-bit)
    0x85, 0x01, 0x0A, 0xD1, 0xE4, 0x15, 0x00, 0x26, 0x10, 0x27, 0x75, 0x10, 0x95, 0x01, 0x55, 0x0E,
    0xB1, 0x02, 0x85, 0x01, 0x0A, 0x04, 0x03, 0x15, 0x00, 0x27, 0xFF, 0xFF, 0xFF, 0xFF, 0x75, 0x20,
    0x95, 0x01, 0x55, 0x00, 0xB1, 0x03, // Minimum report interval (read-only, ms)
    0x85, 0x01, 0x0A, 0x01, 0x02, 0x15, 0x00, 0x25, 0x06, 0x75, 0x08, 0x95, 0x01, 0xA1, 0x02, 0x0A,
    0x00, 0x08, 0x0A, 0x01, 0x08, 0x0A, 0x02, 0x08, 0x0A, 0x03, 0x08, 0x0A, 0x04, 0x08, 0x0A, 0x05,
    0x08, 0x0A, 0x06, 0x08, 0xB1, 0x00, 0xC0, // Sensor State feature
    0xC0,
];

type UsbDriver = Driver<'static, USB>;
type UsbDeviceType = UsbDevice<'static, UsbDriver>;

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDeviceType) -> ! {
    usb.run().await
}

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(spawner: Spawner) {
    static FEATURE_VALUES: critical_section::Mutex<RefCell<FeatureValues>> =
        critical_section::Mutex::new(RefCell::new(FeatureValues::new()));
    static REQUEST_HANDLER: StaticCell<AlsRequestHandler> = StaticCell::new();
    static USB_CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static USB_BOS_DESCRIPTOR: StaticCell<[u8; 128]> = StaticCell::new();
    static USB_CONTROL_BUFFER: StaticCell<[u8; 64]> = StaticCell::new();
    static CDC_STATE: StaticCell<CdcState> = StaticCell::new();
    static HID_STATE: StaticCell<HidState> = StaticCell::new();

    let peripherals = embassy_rp::init(Default::default());

    let mut i2c_config = i2c::Config::default();
    i2c_config.frequency = 400_000;
    let mut sensor = i2c::I2c::new_async(
        peripherals.I2C0,
        peripherals.PIN_1, // SCL, GP1
        peripherals.PIN_0, // SDA, GP0
        Irqs,
        i2c_config,
    );

    let mut usb_config = UsbConfig::new(0x2E8A, 0x10C1);
    usb_config.manufacturer = Some("Adaptive Brightness");
    usb_config.product = Some("RP2040 Ambient Light Sensor");
    usb_config.serial_number = Some("0001");
    usb_config.device_class = 0xEF;
    usb_config.device_sub_class = 0x02;
    usb_config.device_protocol = 0x01;
    usb_config.composite_with_iads = true;
    usb_config.max_packet_size_0 = 64;
    usb_config.max_power = 200;

    let usb_driver = Driver::new(peripherals.USB, Irqs);
    let mut builder = Builder::new(
        usb_driver,
        usb_config,
        USB_CONFIG_DESCRIPTOR.init([0; 256]),
        USB_BOS_DESCRIPTOR.init([0; 128]),
        &mut [],
        USB_CONTROL_BUFFER.init([0; 64]),
    );

    let mut cdc = CdcAcmClass::new(&mut builder, CDC_STATE.init(CdcState::new()), 64);

    let request_handler = REQUEST_HANDLER.init(AlsRequestHandler {
        values: &FEATURE_VALUES,
    });
    let hid_config = HidConfig {
        report_descriptor: ALS_REPORT_DESCRIPTOR,
        request_handler: Some(request_handler),
        poll_ms: 10,
        max_packet_size: 5,
        hid_subclass: HidSubclass::No,
        hid_boot_protocol: HidBootProtocol::None,
    };
    let mut hid = HidWriter::<_, 5>::new(&mut builder, HID_STATE.init(HidState::new()), hid_config);

    let usb = builder.build();
    spawner.spawn(usb_task(usb).unwrap());

    let sensor_ready = init_bh1750(&mut sensor).await;
    let mut last_hid_lux: Option<u16> = None;

    loop {
        let interval_ms =
            critical_section::with(|cs| FEATURE_VALUES.borrow(cs).borrow().report_interval_ms);
        Timer::after_millis(u64::from(interval_ms)).await;

        let mut lux = 0;
        let sample_ready = sensor_ready && read_bh1750(&mut sensor, &mut lux).await;

        if sample_ready {
            let mut line = [0_u8; 16];
            let line_len = format_lux_line(lux, &mut line);
            let _ = embassy_time::with_timeout(
                embassy_time::Duration::from_millis(10),
                cdc.write_packet(&line[..line_len]),
            )
            .await;

            let sensitivity_percent_x100 = critical_section::with(|cs| {
                let values = FEATURE_VALUES.borrow(cs).borrow();
                values.change_sensitivity_percent_x100
            });
            let should_report = last_hid_lux
                .map(|last| {
                    if last == 0 {
                        lux != 0
                    } else {
                        u64::from(lux.abs_diff(last)) * 10_000
                            >= u64::from(last) * u64::from(sensitivity_percent_x100)
                    }
                })
                .unwrap_or(true);

            if critical_section::with(|cs| {
                FEATURE_VALUES
                    .borrow(cs)
                    .borrow()
                    .should_report(should_report)
            }) {
                let report = [
                    REPORT_ID_INPUT,
                    SENSOR_STATE_READY,
                    SENSOR_EVENT_DATA_UPDATED,
                    lux as u8,
                    (lux >> 8) as u8,
                ];
                if matches!(
                    embassy_time::with_timeout(
                        embassy_time::Duration::from_millis(10),
                        hid.write(&report)
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    last_hid_lux = Some(lux);
                }
            }
        } else {
            let _ = embassy_time::with_timeout(
                embassy_time::Duration::from_millis(10),
                cdc.write_packet(b"ERROR:BH1750\r\n"),
            )
            .await;
        }
    }
}

async fn init_bh1750<I2C: embedded_hal_async::i2c::I2c>(i2c: &mut I2C) -> bool {
    if i2c.write(BH1750_ADDRESS, &[BH1750_POWER_ON]).await.is_err() {
        return false;
    }
    Timer::after_millis(10).await;
    i2c.write(BH1750_ADDRESS, &[BH1750_CONTINUOUS_HIGH_RES_MODE])
        .await
        .is_ok()
}

async fn read_bh1750<I2C: embedded_hal_async::i2c::I2c>(i2c: &mut I2C, lux: &mut u16) -> bool {
    let mut raw = [0_u8; 2];
    if i2c.read(BH1750_ADDRESS, &mut raw).await.is_err() {
        return false;
    }

    let measurement = u16::from_be_bytes(raw);
    *lux = ((u32::from(measurement) * 10 + 6) / 12) as u16;
    true
}

fn format_lux_line(lux: u16, output: &mut [u8; 16]) -> usize {
    let mut digits = [0_u8; 5];
    let mut value = lux;
    let mut digit_count = 0;
    loop {
        digits[digit_count] = b'0' + (value % 10) as u8;
        digit_count += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }

    output[..4].copy_from_slice(b"LUX:");
    for (index, digit) in digits[..digit_count].iter().rev().enumerate() {
        output[4 + index] = *digit;
    }
    output[4 + digit_count] = b'\r';
    output[5 + digit_count] = b'\n';
    6 + digit_count
}

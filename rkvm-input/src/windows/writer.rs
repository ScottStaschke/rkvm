use crate::abs::{AbsAxis, AbsInfo, AbsEvent};
use crate::event::Event;
use crate::key::{Key, KeyEvent,Keyboard, Button};
use crate::rel::{RelAxis, RelEvent};
use crate::writer::{DeviceWriter, EventWriter, WriterPlatform, WriterBuilderPlatform};

use crate::windows::injector::send_input;
use crate::windows::key_repeater::KeyRepeater;
use crate::windows::normalizer::AxisNormalizer;
use crate::windows::writer_simple::WriterWindowsSimple;

use async_trait::async_trait;
use std::ffi::CString;
use std::io::{Error, ErrorKind};
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::Entry;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::*;


pub struct WritersWindows<W> {
    writer: W,
    dev: HashMap<usize,DeviceWriterWindows>,
}

struct DeviceWriterWindows {
    repeat_delay: Duration,
    repeat_period: Duration,
    key_reapeter: Option<KeyRepeater>,

    hi_wheel: bool,
    hi_hwheel: bool,
    abs_norm: HashMap<AbsAxis, AxisNormalizer>,
}

impl<W> WritersWindows<W>
where W: EventWriter + Send {
    pub fn new(writer: W) -> Self {
        WritersWindows { writer: writer, dev: HashMap::new() }
    }
}

impl DeviceWriterWindows {
    pub async fn event(&mut self, writer: &mut impl EventWriter, id: usize, event: Event) -> Result<(),Error> {
        match event {
            Event::Rel(RelEvent { axis, value: _ }) => {
                match axis {
                    RelAxis::X => writer.event(id, event).await?,
                    RelAxis::Y => writer.event(id, event).await?,
                    RelAxis::Wheel => if !self.hi_wheel {
                        writer.event(id, event).await?
                    },
                    RelAxis::HWheel => if !self.hi_hwheel {
                        writer.event(id, event).await?
                    },
                    RelAxis::WheelHiRes => if self.hi_wheel {
                        writer.event(id, event).await?
                    },
                    RelAxis::HWheelHiRes => if self.hi_hwheel {
                        writer.event(id, event).await?
                    },
                    _ => tracing::info!("Axis not handled {:?}", event),
                }
            }
            Event::Abs(event) => {
                match event {
                    AbsEvent::Axis { axis, value } => {
                         if let Some(normalizer) = self.abs_norm.get(&axis) {
                            let nv = normalizer.normalize(value);
                            writer.event(id, Event::Abs(AbsEvent::Axis { axis: axis, value: nv})).await?
                        } else {
                            tracing::warn!("Abs Axis not handled: {:?}", axis);
                        }
                    },
                    _ => tracing::warn!("Abs event not handled: {:?}", event),
                }
            }
            _ => {
                writer.event(id, event).await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<W> DeviceWriter for WritersWindows<W>
where W: EventWriter +Send {
    async fn create_device(&mut self, id: usize, _name: &CString, _vendor: u16, _product: u16, _version: u16, rel: HashSet<RelAxis>, abs: HashMap<AbsAxis, AbsInfo>, _keys: HashSet<Key>, delay: Option<i32>, period: Option<i32>) -> Result<(), Error> {
        let entry = self.dev.entry(id);
        if let Entry::Occupied(_) = entry {
            return Err(Error::new(ErrorKind::InvalidData, "Server created the same device twice"));
        }

        let mut hi_wheel = false;
        let mut hi_hwheel = false;
        let mut abs_norm = HashMap::new();

        for axis in rel {
            match axis {
                RelAxis::WheelHiRes => hi_wheel = true,
                RelAxis::HWheelHiRes => hi_hwheel = true,
                _ => {},
            }
        }

        abs.into_iter().for_each(|(axis, info)| {
            let normalizer = AxisNormalizer::new(info.min, info.max);
            abs_norm.insert(axis, normalizer);
        });

        let repeat_delay = Duration::from_millis(delay.map_or(0, |x| if x<0 { 0 } else { x }) as u64);
        let repeat_period = Duration::from_millis(period.map_or(0, |x| if x<0  { 0 } else { x }) as u64);

        entry.or_insert(DeviceWriterWindows {
            repeat_delay: repeat_delay,
            repeat_period: repeat_period,
            key_reapeter: None,

            hi_wheel: hi_wheel,
            hi_hwheel: hi_hwheel,
            abs_norm: abs_norm,
        });

        Ok(())
    }
    async fn destroy_device(&mut self, id: usize) -> Result<(), Error> {
        if self.dev.remove(&id).is_none() {
            return Err(Error::new(ErrorKind::InvalidData, "Server destroyed a nonexistent device"));
        }

        tracing::info!(id = %id, "Destroyed device");
        Ok(())
    }
}

#[async_trait]
impl<W> EventWriter for WritersWindows<W>
where W: EventWriter +Send {
    async fn event(&mut self, id: usize, event: Event) -> Result<(), Error> {
        let dev = self.dev.get_mut(&id).ok_or_else(|| { Error::new(ErrorKind::InvalidData,"Server sent an event to a nonexistent device",)})?;
        dev.event(&mut self.writer, id, event).await
    }
}

// XXX
pub struct WriterWindows {
    repeat_delay: Duration,
    repeat_period: Duration,
    key_reapeter: Option<KeyRepeater>,

    hi_wheel: bool,
    hi_hwheel: bool,
    abs_norm: HashMap<AbsAxis, AxisNormalizer>,
}

impl WriterWindows {
    pub fn key(&mut self, key: &Keyboard, down:&bool) {
        if let Some((scan, extended)) = map_key_to_scancode(key) {
            let mut flags = KEYEVENTF_SCANCODE;
            if !down { flags |= KEYEVENTF_KEYUP; }
            if extended { flags |= KEYEVENTF_EXTENDEDKEY; }
            
            match (&mut self.key_reapeter,down) {
                (Some(kr), _) => {
                    if kr.key(*key, flags, scan, down) {
                        self.key_reapeter = None;
                    }
                }
                (None, true) => self.key_reapeter = Some(KeyRepeater::new(*key, scan, flags, self.repeat_delay, self.repeat_period)),
                (_,_) => {}
            }

            send_input(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY::default(),
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }

    fn button(&mut self, button: &Button, down:&bool) {
        if let Some((flags, mousedata)) = map_button(button, down) {
            send_input(INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 0,
                        dy: 0,
                        mouseData: mousedata,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            })
        }
    }

    fn mouse_move(&mut self, abs: bool, dx: i32, dy: i32) {
        let mut flags = MOUSEEVENTF_MOVE;
        if abs {
            flags |= MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        }
        send_input(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
    }

    fn mouse_wheel(&mut self, delta: i32) {
         send_input(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: delta as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
    }

      fn mouse_hwheel(&mut self, delta: i32) {
         send_input(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: delta as u32,
                    dwFlags: MOUSEEVENTF_HWHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
    }
}

impl WriterPlatform for WriterWindows {
    type Builder = WriterWindowsBuilder;
    fn builder() -> Result<Self::Builder, Error> {
        Ok(WriterWindowsBuilder::new())
    }

    async fn write(&mut self, event: &Event) -> Result<(), Error> {
        match event {
            Event::Key(KeyEvent { key, down }) => {
                match key {
                    Key::Key(key) => self.key(key, down),
                    Key::Button(button) => self.button(button, down),
                }
            }
            Event::Rel(RelEvent { axis, value }) => {
                match axis {
                    RelAxis::X => self.mouse_move(false, *value, 0),
                    RelAxis::Y => self.mouse_move(false, 0, *value),
                    RelAxis::Wheel => if !self.hi_wheel {
                        self.mouse_wheel(*value*120)
                    }
                    RelAxis::HWheel => if !self.hi_hwheel {
                        self.mouse_hwheel(*value*120)
                    }
                    RelAxis::WheelHiRes => if self.hi_wheel {
                        self.mouse_wheel(*value)
                    }
                    RelAxis::HWheelHiRes => if self.hi_hwheel {
                        self.mouse_hwheel(*value)
                    }
                    _ => tracing::warn!("Axe not handled: {:?}", axis),
                }
            }
            Event::Abs(event) => {
                match event {
                    AbsEvent::Axis { axis, value } => {
                         if let Some(normalizer) = self.abs_norm.get(axis) {
                            let nv = normalizer.normalize(*value);
                            match axis {
                                AbsAxis::X => self.mouse_move(true, nv, 0),
                                AbsAxis::Y => self.mouse_move(true, 0, nv),
                                _ => tracing::warn!("Abs Axis not handled: {:?}", axis)
                            }
                        } else {
                            tracing::warn!("Abs Axis not handled: {:?}", axis);
                        }
                    }
                    _ => tracing::warn!("Abs event not handled: {:?}", event),
                }
            }
            _ => {}
        }

        Ok(())
    }
}

pub struct WriterWindowsBuilder {
    hi_wheel: bool,
    hi_hwheel: bool,
    abs_norm: HashMap<AbsAxis, AxisNormalizer>,
    repeat_delay: Duration,
    repeat_period: Duration,
}

impl WriterWindowsBuilder {
     fn new() -> Self {
        Self {
            hi_wheel: false,
            hi_hwheel: false,
            abs_norm: HashMap::new(),
            repeat_delay: Duration::ZERO,
            repeat_period: Duration::ZERO,
        }
    }
}

impl WriterBuilderPlatform for WriterWindowsBuilder {
    type Writer = WriterWindows;

    fn name(self, _name: &CString) -> Self {
        self
    }

    fn vendor(self, _value: u16) -> Self {
        self
    }

    fn product(self, _value: u16) -> Self {
        self
    }

    fn version(self, _value: u16) -> Self {
        self
    }
    fn rel<T: IntoIterator<Item = RelAxis>>(mut self, items: T) -> Result<Self, Error> {
        for axis in items {
            match axis {
                RelAxis::WheelHiRes => self.hi_wheel = true,
                RelAxis::HWheelHiRes => self.hi_hwheel = true,
                _ => {},
            }
        }
        Ok(self)
    }
    fn abs<T: IntoIterator<Item = (AbsAxis, AbsInfo)>>(mut self, items: T) -> Result<Self, Error> {
        items.into_iter().for_each(|(axis, info)| {
            let normalizer = AxisNormalizer::new(info.min, info.max);
            self.abs_norm.insert(axis, normalizer);
        });
        Ok(self)
    }
    fn key<T: IntoIterator<Item = Key>>(self, _items: T) -> Result<Self, Error> {
        Ok(self)
    }

    fn delay(mut self, value: Option<i32>) -> Result<Self, Error> {
        if let Some(delay) = value {
            if delay > 0 {
                self.repeat_delay = Duration::from_millis(delay as u64);
            }
        }
        Ok(self)
    }

    fn period(mut self, value: Option<i32>) -> Result<Self, Error> {
        if let Some(period) = value {
            if period > 0 {
                self.repeat_period = Duration::from_millis(period as u64);
            }
        }
        Ok(self)
    }

    async fn build(self) -> Result<Self::Writer, Error> {
        Ok(WriterWindows{
            hi_wheel: self.hi_wheel,
            hi_hwheel: self.hi_hwheel,
            abs_norm: self.abs_norm,
            repeat_delay: self.repeat_delay,
            repeat_period: self.repeat_period,
            key_reapeter: None,
        })
    }
}

fn map_button(button: &Button, down:&bool) -> Option<(MOUSE_EVENT_FLAGS,u32)>  {
   match button {
        Button::Left => Some((
            if *down { MOUSEEVENTF_LEFTDOWN } else { MOUSEEVENTF_LEFTUP },
            0 as u32,
        )),
        Button::Right => Some((
            if *down { MOUSEEVENTF_RIGHTDOWN } else { MOUSEEVENTF_RIGHTUP },
            0 as u32,
        )),
        Button::Middle => Some((
            if *down { MOUSEEVENTF_MIDDLEDOWN } else { MOUSEEVENTF_MIDDLEUP },
            0 as u32,
        )),
        Button::Side => Some((
            if *down { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP },
            1 as u32,
        )),
        Button::Extra => Some((
            if *down { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP },
            2 as u32,
        )),
        _ => {
            tracing::warn!("Unsupported mouse button: {:?}", button);
            None
        }
    }
}

fn map_key_to_scancode(key: &Keyboard) -> Option<(u16, bool)> {
    match key {
        // Letters
        Keyboard::A => Some((0x1E, false)),
        Keyboard::B => Some((0x30, false)),
        Keyboard::C => Some((0x2E, false)),
        Keyboard::D => Some((0x20, false)),
        Keyboard::E => Some((0x12, false)),
        Keyboard::F => Some((0x21, false)),
        Keyboard::G => Some((0x22, false)),
        Keyboard::H => Some((0x23, false)),
        Keyboard::I => Some((0x17, false)),
        Keyboard::J => Some((0x24, false)),
        Keyboard::K => Some((0x25, false)),
        Keyboard::L => Some((0x26, false)),
        Keyboard::M => Some((0x32, false)),
        Keyboard::N => Some((0x31, false)),
        Keyboard::O => Some((0x18, false)),
        Keyboard::P => Some((0x19, false)),
        Keyboard::Q => Some((0x10, false)),
        Keyboard::R => Some((0x13, false)),
        Keyboard::S => Some((0x1F, false)),
        Keyboard::T => Some((0x14, false)),
        Keyboard::U => Some((0x16, false)),
        Keyboard::V => Some((0x2F, false)),
        Keyboard::W => Some((0x11, false)),
        Keyboard::X => Some((0x2D, false)),
        Keyboard::Y => Some((0x15, false)),
        Keyboard::Z => Some((0x2C, false)),

        // Numbers
        Keyboard::N1 => Some((0x02, false)),
        Keyboard::N2 => Some((0x03, false)),
        Keyboard::N3 => Some((0x04, false)),
        Keyboard::N4 => Some((0x05, false)),
        Keyboard::N5 => Some((0x06, false)),
        Keyboard::N6 => Some((0x07, false)),
        Keyboard::N7 => Some((0x08, false)),
        Keyboard::N8 => Some((0x09, false)),
        Keyboard::N9 => Some((0x0A, false)),
        Keyboard::N0 => Some((0x0B, false)),

        // Arrows
        Keyboard::Up => Some((0x48, true)),
        Keyboard::Down => Some((0x50, true)),
        Keyboard::Left => Some((0x4B, true)),
        Keyboard::Right => Some((0x4D, true)),

        // Functions
        Keyboard::F1 => Some((0x3B, false)),
        Keyboard::F2 => Some((0x3C, false)),
        Keyboard::F3 => Some((0x3D, false)),
        Keyboard::F4 => Some((0x3E, false)),
        Keyboard::F5 => Some((0x3F, false)),
        Keyboard::F6 => Some((0x40, false)),
        Keyboard::F7 => Some((0x41, false)),
        Keyboard::F8 => Some((0x42, false)),
        Keyboard::F9 => Some((0x43, false)),
        Keyboard::F10 => Some((0x44, false)),
        Keyboard::F11 => Some((0x57, false)),
        Keyboard::F12 => Some((0x58, false)),

      

        // Special Keyboards
        Keyboard::Enter => Some((0x1C, false)),
        Keyboard::Minus => Some((0x0C, false)),
        Keyboard::Equal => Some((0x0D, false)),
        Keyboard::LeftBrace => Some((0x1A, false)),
        Keyboard::RightBrace => Some((0x1B, false)),
        Keyboard::Apostrophe => Some((0x28, false)),
        Keyboard::Slash => Some((0x35, false)),
        Keyboard::Dot => Some((0x34, false)),
        Keyboard::Semicolon => Some((0x27, false)),
        Keyboard::Grave => Some((0x29, false)),
        Keyboard::Comma => Some((0x33, false)),
        Keyboard::Backslash => Some((0x2B, false)),
        Keyboard::Esc => Some((0x01, false)),
        Keyboard::Backspace => Some((0x0E, false)),
        Keyboard::Tab => Some((0x0F, false)),
        Keyboard::Space => Some((0x39, false)),
        Keyboard::CapsLock => Some((0x3A, false)),
        Keyboard::LeftShift => Some((0x2A, false)),
        Keyboard::RightShift => Some((0x36, false)),
        Keyboard::LeftCtrl => Some((0x1D, false)),
        Keyboard::RightCtrl => Some((0x1D, true)),
        Keyboard::LeftAlt => Some((0x38, false)),
        Keyboard::RightAlt => Some((0x38, true)),
        Keyboard::LeftMeta => Some((0x5B, true)), // Windows Keyboard
        Keyboard::RightMeta => Some((0x5C, true)),
        Keyboard::SysRq => Some((0x54, false)),
        Keyboard::ScrollLock => Some((0x46, false)),
        Keyboard::Compose => Some((0x5D, true)),
        Keyboard::Pause => Some((0x45, true)),
        Keyboard::N102nd => Some((0x56, false)),

        Keyboard::Insert => Some((0x52, true)),
        Keyboard::Delete => Some((0x53, true)),
        Keyboard::Home => Some((0x47, true)),
        Keyboard::End => Some((0x4F, true)),
        Keyboard::PageUp => Some((0x49, true)),
        Keyboard::PageDown => Some((0x51, true)),

        // keypad
        Keyboard::NumLock => Some((0x45, false)),
        Keyboard::Kp0 => Some((0x52, false)),
        Keyboard::Kp1 => Some((0x4F, false)),
        Keyboard::Kp2 => Some((0x50, false)),
        Keyboard::Kp3 => Some((0x51, false)),
        Keyboard::Kp4 => Some((0x4B, false)),
        Keyboard::Kp5 => Some((0x4C, false)),
        Keyboard::Kp6 => Some((0x4D, false)),
        Keyboard::Kp7 => Some((0x47, false)),
        Keyboard::Kp8 => Some((0x48, false)),
        Keyboard::Kp9 => Some((0x49, false)),
        Keyboard::KpDot => Some((0x53, false)),
        Keyboard::KpAsterisk => Some((0x37, false)),
        Keyboard::KpEnter => Some((0x1C, true)),
        Keyboard::KpMinus => Some((0x4A, false)),
        Keyboard::KpPlus => Some((0x4E, false)),
        Keyboard::KpSlash => Some((0x35, true)),

        _ => {
            tracing::warn!("Unsupported keyboard key : {:?}", key);
            None
        }
    }
}

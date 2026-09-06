//! Visible attended-session input on the ordinary interactive desktop only.
use crate::desktop::{interactive_desktop, DesktopError};
use sensor_media::{Display, Input, MouseButton};
use sensor_session::permissions::{Consent, Permission};
use std::collections::BTreeSet;
use windows::Win32::UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*};

fn keyboard(key: u16, down: bool) -> INPUT {
    // Extended keys need their prefix; printable text uses Unicode below.
    let extended = matches!(
        key,
        0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x6F | 0x90 | 0xA3 | 0xA5
    );
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(key),
                dwFlags: if down {
                    KEYBD_EVENT_FLAGS(0)
                } else {
                    KEYEVENTF_KEYUP
                } | if extended {
                    KEYEVENTF_EXTENDEDKEY
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    }
}
fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}
fn button_flag(button: MouseButton, down: bool) -> MOUSE_EVENT_FLAGS {
    match (button, down) {
        (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
        (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
        (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
        (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
        (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
        (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
    }
}
fn send(inputs: &[INPUT]) -> Result<(), DesktopError> {
    if inputs.is_empty() {
        return Ok(());
    }
    interactive_desktop()?;
    if unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) } != inputs.len() as u32 {
        // UIPI refusal is a failure, never a claim of secure-desktop/elevated control.
        return Err(windows::core::Error::from_win32().into());
    }
    Ok(())
}

#[derive(Default)]
pub struct Injector {
    keys: BTreeSet<u16>,
    buttons: Vec<MouseButton>,
}
impl Injector {
    pub fn apply(
        &mut self,
        event: &Input,
        display: &Display,
        consent: &Consent,
    ) -> Result<(), DesktopError> {
        consent.require(Permission::Input)?;
        event.validate(display)?;
        interactive_desktop()?;
        match event {
            Input::Move { x, y } => {
                let left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
                let top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
                let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
                let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
                if width <= 1 || height <= 1 {
                    return Err(DesktopError::Monitor);
                }
                let dx = ((display.left as i64 + *x as i64 - left as i64) * 65535
                    / (width as i64 - 1))
                    .clamp(0, 65535) as i32;
                let dy = ((display.top as i64 + *y as i64 - top as i64) * 65535
                    / (height as i64 - 1))
                    .clamp(0, 65535) as i32;
                send(&[mouse(
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    dx,
                    dy,
                    0,
                )])?;
            }
            Input::Button { button, down } => {
                send(&[mouse(button_flag(*button, *down), 0, 0, 0)])?;
                self.buttons.retain(|b| b != button);
                if *down {
                    self.buttons.push(*button);
                }
            }
            Input::Wheel { delta, horizontal } => send(&[mouse(
                if *horizontal {
                    MOUSEEVENTF_HWHEEL
                } else {
                    MOUSEEVENTF_WHEEL
                },
                0,
                0,
                *delta as u32,
            )])?,
            Input::Key { virtual_key, down } => {
                send(&[keyboard(*virtual_key, *down)])?;
                if *down {
                    self.keys.insert(*virtual_key);
                } else {
                    self.keys.remove(virtual_key);
                }
            }
            Input::Text(text) => {
                let mut events = Vec::new();
                for code in text.encode_utf16() {
                    for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
                        events.push(INPUT {
                            r#type: INPUT_KEYBOARD,
                            Anonymous: INPUT_0 {
                                ki: KEYBDINPUT {
                                    wScan: code,
                                    dwFlags: flags,
                                    ..Default::default()
                                },
                            },
                        });
                    }
                }
                send(&events)?;
            }
            Input::ReleaseAll => self.release()?,
        }
        Ok(())
    }
    pub fn release(&mut self) -> Result<(), DesktopError> {
        let mut events: Vec<_> = self.keys.iter().map(|key| keyboard(*key, false)).collect();
        events.extend(
            self.buttons
                .iter()
                .map(|button| mouse(button_flag(*button, false), 0, 0, 0)),
        );
        let result = send(&events);
        self.keys.clear();
        self.buttons.clear();
        result
    }
}
impl Drop for Injector {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sensor_session::permissions::Permissions;
    #[test]
    fn rejected_or_view_only_input_never_reaches_the_os() {
        let display = Display {
            index: 0,
            name: "test".into(),
            left: 0,
            top: 0,
            width: 100,
            height: 100,
        };
        let mut injector = Injector::default();
        let mut consent = Consent::pending(Permissions::screen_sharing());
        assert!(matches!(
            injector.apply(&Input::Move { x: 1, y: 1 }, &display, &consent),
            Err(DesktopError::Permission(_))
        ));
        consent.accept(Permissions::screen_sharing()).unwrap();
        assert!(matches!(
            injector.apply(
                &Input::Key {
                    virtual_key: 65,
                    down: true
                },
                &display,
                &consent
            ),
            Err(DesktopError::Permission(_))
        ));
        assert!(injector.keys.is_empty());
    }
}

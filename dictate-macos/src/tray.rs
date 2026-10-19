//! The menu-bar item and its menu.
//!
//! Icons are drawn at runtime, one per state, so the repository carries no
//! image files and the colours live next to the states they mean. The menu is
//! rebuilt whenever what it shows changes, because macOS opens it without
//! asking the application first.

use dictate_core::SessionState;
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const ICON_SIZE: u32 = 32;

#[derive(Clone)]
pub enum MenuChoice {
    /// The text of a recent utterance, to copy.
    Recent(String),
    /// None: the system default input. Some: an input's name.
    Microphone(Option<String>),
    OpenConfigFolder,
    ReEnterKeys,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    choices: Vec<(MenuId, MenuChoice)>,
}

impl Tray {
    pub fn new() -> Result<Self, String> {
        let icon = TrayIconBuilder::new()
            .with_icon(icon_for(SessionState::Idle)?)
            .with_tooltip("dictate — idle")
            .build()
            .map_err(|error| format!("Could not create the menu-bar item: {error}"))?;

        Ok(Self { icon, choices: Vec::new() })
    }

    pub fn set_state(&mut self, state: SessionState) {
        if let Ok(icon) = icon_for(state) {
            let _ = self.icon.set_icon(Some(icon));
        }
        let _ = self.icon.set_tooltip(Some(format!("dictate — {}", state.name())));
    }

    pub fn choice(&self, id: &MenuId) -> Option<MenuChoice> {
        self.choices.iter().find(|(candidate, _)| candidate == id).map(|(_, choice)| choice.clone())
    }

    /// `selected` is the input in use, None for the system default.
    pub fn rebuild(&mut self, recent: &[String], inputs: &[String], selected: Option<usize>) {
        let mut choices = Vec::new();
        let menu = Menu::new();

        let recent_menu = Submenu::new("Recent", !recent.is_empty());
        for text in recent {
            let item = MenuItem::new(menu_label(text), true, None);
            choices.push((item.id().clone(), MenuChoice::Recent(text.clone())));
            let _ = recent_menu.append(&item);
        }
        let _ = menu.append(&recent_menu);

        let microphone_menu = Submenu::new("Microphone", true);
        let default_item = CheckMenuItem::new("System default", true, selected.is_none(), None);
        choices.push((default_item.id().clone(), MenuChoice::Microphone(None)));
        let _ = microphone_menu.append(&default_item);
        let _ = microphone_menu.append(&PredefinedMenuItem::separator());
        for (index, name) in inputs.iter().enumerate() {
            let item = CheckMenuItem::new(name, true, selected == Some(index), None);
            choices.push((item.id().clone(), MenuChoice::Microphone(Some(name.clone()))));
            let _ = microphone_menu.append(&item);
        }
        let _ = menu.append(&microphone_menu);

        let _ = menu.append(&PredefinedMenuItem::separator());
        for (label, choice) in [
            ("Open config folder", MenuChoice::OpenConfigFolder),
            ("Re-enter API keys…", MenuChoice::ReEnterKeys),
        ] {
            let item = MenuItem::new(label, true, None);
            choices.push((item.id().clone(), choice));
            let _ = menu.append(&item);
        }
        let _ = menu.append(&PredefinedMenuItem::separator());
        let quit = MenuItem::new("Quit dictate", true, None);
        choices.push((quit.id().clone(), MenuChoice::Quit));
        let _ = menu.append(&quit);

        self.icon.set_menu(Some(Box::new(menu)));
        self.choices = choices;
    }
}

/// First 60 characters, one line.
fn menu_label(text: &str) -> String {
    let flat: String = text.chars().map(|c| if c == '\r' || c == '\n' { ' ' } else { c }).collect();
    let mut label: String = flat.chars().take(60).collect();
    if flat.chars().count() > 60 {
        label.push('…');
    }
    label
}

fn icon_for(state: SessionState) -> Result<Icon, String> {
    let (r, g, b) = match state {
        SessionState::Idle => (148, 163, 184),        // slate
        SessionState::Recording => (239, 68, 68),     // red
        SessionState::Transcribing => (59, 130, 246), // blue
    };
    Icon::from_rgba(draw_circle(r, g, b), ICON_SIZE, ICON_SIZE).map_err(|error| error.to_string())
}

/// A filled circle with a soft white rim, RGBA with straight alpha,
/// anti-aliased by coverage.
fn draw_circle(r: u8, g: u8, b: u8) -> Vec<u8> {
    const RADIUS: f32 = 12.0;
    let size = ICON_SIZE as usize;
    let centre = size as f32 / 2.0;
    let mut rgba = vec![0u8; size * size * 4];

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - centre;
            let dy = y as f32 + 0.5 - centre;
            let distance = (dx * dx + dy * dy).sqrt();

            let fill = (RADIUS + 0.5 - distance).clamp(0.0, 1.0);
            let rim = (1.5 - (distance - RADIUS).abs()).clamp(0.0, 1.0) * (200.0 / 255.0);
            let alpha = rim + fill * (1.0 - rim);

            let blend = |channel: u8| -> u8 {
                if alpha <= 0.0 {
                    0
                } else {
                    ((255.0 * rim + f32::from(channel) * fill * (1.0 - rim)) / alpha).round() as u8
                }
            };

            let i = (y * size + x) * 4;
            rgba[i] = blend(r);
            rgba[i + 1] = blend(g);
            rgba[i + 2] = blend(b);
            rgba[i + 3] = (alpha * 255.0).round() as u8;
        }
    }

    rgba
}

use crate::overlay::{
    AnimationType, BorderSettings, ColorSettings, KeycapStyle, TextCaps, TextSettings,
    TextVariant,
};
use serde::{Deserialize, Serialize};

/// A named color scheme preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemePreset {
    pub name: String,
    pub colors: ColorSettings,
    pub text: TextSettings,
    pub border: BorderSettings,
    pub keycap_style: KeycapStyle,
}

impl ThemePreset {
    pub fn all() -> Vec<Self> {
        vec![
            charcoal(),
            silver(),
            cyber(),
            eclipse(),
            lime(),
            turquoise(),
            blue(),
            yellow(),
            green(),
            pink(),
            red(),
            pansy(),
            bumblebee(),
            stone(),
        ]
    }

    pub fn by_name(name: &str) -> Option<Self> {
        Self::all().into_iter().find(|p| p.name == name)
    }

    pub fn name_list() -> Vec<String> {
        Self::all().iter().map(|p| p.name.clone()).collect()
    }
}

fn charcoal() -> ThemePreset {
    ThemePreset {
        name: "Charcoal".into(),
        colors: ColorSettings {
            keycap_primary: "#404040".into(),
            keycap_secondary: "#2e2e2e".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#404040".into(),
            modifier_secondary: "#2e2e2e".into(),
        },
        text: TextSettings {
            size: None,
            color: "#FFFFFF".into(),
            modifier_color: "#FFFFFF".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#555555".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#555555".into(),
        },
        keycap_style: KeycapStyle::Laptop,
    }
}

fn silver() -> ThemePreset {
    ThemePreset {
        name: "Silver".into(),
        colors: ColorSettings {
            keycap_primary: "#f8f8f8".into(),
            keycap_secondary: "#dcdcdc".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#f8f8f8".into(),
            modifier_secondary: "#dcdcdc".into(),
        },
        text: TextSettings {
            size: None,
            color: "#000000".into(),
            modifier_color: "#000000".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#c0c0c0".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#c0c0c0".into(),
        },
        keycap_style: KeycapStyle::Laptop,
    }
}

fn cyber() -> ThemePreset {
    ThemePreset {
        name: "Cyber".into(),
        colors: ColorSettings {
            keycap_primary: "#00B1D2".into(),
            keycap_secondary: "#008ea8".into(),
            use_gradient: true,
            highlight_modifiers: true,
            modifier_primary: "#FDDB27".into(),
            modifier_secondary: "#dfc019".into(),
        },
        text: TextSettings {
            size: None,
            color: "#FFFFFF".into(),
            modifier_color: "#000000".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#009ab5".into(),
            width: 1.0,
            radius: 0.3,
            modifier_color: "#d4b020".into(),
        },
        keycap_style: KeycapStyle::PBT,
    }
}

fn eclipse() -> ThemePreset {
    ThemePreset {
        name: "Eclipse".into(),
        colors: ColorSettings {
            keycap_primary: "#343148".into(),
            keycap_secondary: "#252333".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#343148".into(),
            modifier_secondary: "#252333".into(),
        },
        text: TextSettings {
            size: None,
            color: "#D7C49E".into(),
            modifier_color: "#D7C49E".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#4a4660".into(),
            width: 1.0,
            radius: 0.3,
            modifier_color: "#4a4660".into(),
        },
        keycap_style: KeycapStyle::PBT,
    }
}

fn lime() -> ThemePreset {
    ThemePreset {
        name: "Lime".into(),
        colors: ColorSettings {
            keycap_primary: "#606060".into(),
            keycap_secondary: "#4b4b4b".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#606060".into(),
            modifier_secondary: "#4b4b4b".into(),
        },
        text: TextSettings {
            size: None,
            color: "#D6ED17".into(),
            modifier_color: "#D6ED17".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#777777".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#777777".into(),
        },
        keycap_style: KeycapStyle::Laptop,
    }
}

fn turquoise() -> ThemePreset {
    ThemePreset {
        name: "Turquoise".into(),
        colors: ColorSettings {
            keycap_primary: "#42EADD".into(),
            keycap_secondary: "#2ec4b8".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#42EADD".into(),
            modifier_secondary: "#2ec4b8".into(),
        },
        text: TextSettings {
            size: None,
            color: "#FFFFFF".into(),
            modifier_color: "#FFFFFF".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#38d4c5".into(),
            width: 1.0,
            radius: 0.3,
            modifier_color: "#38d4c5".into(),
        },
        keycap_style: KeycapStyle::PBT,
    }
}

fn blue() -> ThemePreset {
    ThemePreset {
        name: "Blue".into(),
        colors: ColorSettings {
            keycap_primary: "#2196f3".into(),
            keycap_secondary: "#1976d2".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#2196f3".into(),
            modifier_secondary: "#1976d2".into(),
        },
        text: TextSettings {
            size: None,
            color: "#FFFFFF".into(),
            modifier_color: "#FFFFFF".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#1e88e5".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#1e88e5".into(),
        },
        keycap_style: KeycapStyle::Laptop,
    }
}

fn yellow() -> ThemePreset {
    ThemePreset {
        name: "Yellow".into(),
        colors: ColorSettings {
            keycap_primary: "#FDDB27".into(),
            keycap_secondary: "#dfc019".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#FDDB27".into(),
            modifier_secondary: "#dfc019".into(),
        },
        text: TextSettings {
            size: None,
            color: "#000000".into(),
            modifier_color: "#000000".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#c4a810".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#c4a810".into(),
        },
        keycap_style: KeycapStyle::Laptop,
    }
}

fn green() -> ThemePreset {
    ThemePreset {
        name: "Green".into(),
        colors: ColorSettings {
            keycap_primary: "#66bb6a".into(),
            keycap_secondary: "#43a047".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#66bb6a".into(),
            modifier_secondary: "#43a047".into(),
        },
        text: TextSettings {
            size: None,
            color: "#FFFFFF".into(),
            modifier_color: "#FFFFFF".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#52b156".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#52b156".into(),
        },
        keycap_style: KeycapStyle::Laptop,
    }
}

fn pink() -> ThemePreset {
    ThemePreset {
        name: "Pink".into(),
        colors: ColorSettings {
            keycap_primary: "#f06292".into(),
            keycap_secondary: "#d81b60".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#f06292".into(),
            modifier_secondary: "#d81b60".into(),
        },
        text: TextSettings {
            size: None,
            color: "#FFFFFF".into(),
            modifier_color: "#FFFFFF".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#e04e80".into(),
            width: 1.0,
            radius: 0.3,
            modifier_color: "#e04e80".into(),
        },
        keycap_style: KeycapStyle::PBT,
    }
}

fn red() -> ThemePreset {
    ThemePreset {
        name: "Red".into(),
        colors: ColorSettings {
            keycap_primary: "#ef5350".into(),
            keycap_secondary: "#c62828".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#ef5350".into(),
            modifier_secondary: "#c62828".into(),
        },
        text: TextSettings {
            size: None,
            color: "#FFFFFF".into(),
            modifier_color: "#FFFFFF".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#d84040".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#d84040".into(),
        },
        keycap_style: KeycapStyle::Laptop,
    }
}

fn pansy() -> ThemePreset {
    ThemePreset {
        name: "Pansy".into(),
        colors: ColorSettings {
            keycap_primary: "#673ab7".into(),
            keycap_secondary: "#4527a0".into(),
            use_gradient: true,
            highlight_modifiers: true,
            modifier_primary: "#ffc107".into(),
            modifier_secondary: "#ffab00".into(),
        },
        text: TextSettings {
            size: None,
            color: "#FFFFFF".into(),
            modifier_color: "#000000".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#5e35b1".into(),
            width: 1.0,
            radius: 0.3,
            modifier_color: "#e6ac00".into(),
        },
        keycap_style: KeycapStyle::PBT,
    }
}

fn bumblebee() -> ThemePreset {
    ThemePreset {
        name: "Bumblebee".into(),
        colors: ColorSettings {
            keycap_primary: "#404040".into(),
            keycap_secondary: "#2e2e2e".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#404040".into(),
            modifier_secondary: "#2e2e2e".into(),
        },
        text: TextSettings {
            size: None,
            color: "#FDDB27".into(),
            modifier_color: "#FDDB27".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#555555".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#555555".into(),
        },
        keycap_style: KeycapStyle::Laptop,
    }
}

fn stone() -> ThemePreset {
    ThemePreset {
        name: "Stone".into(),
        colors: ColorSettings {
            keycap_primary: "#606060".into(),
            keycap_secondary: "#4b4b4b".into(),
            use_gradient: true,
            highlight_modifiers: false,
            modifier_primary: "#606060".into(),
            modifier_secondary: "#4b4b4b".into(),
        },
        text: TextSettings {
            size: None,
            color: "#f8f8f8".into(),
            modifier_color: "#f8f8f8".into(),
            caps: TextCaps::Capitalize,
            variant: TextVariant::Short,
        },
        border: BorderSettings {
            enabled: true,
            color: "#777777".into(),
            width: 1.0,
            radius: 0.25,
            modifier_color: "#777777".into(),
        },
        keycap_style: KeycapStyle::Laptop,
    }
}

/// Default animation settings.
pub fn default_animation_type() -> AnimationType {
    AnimationType::Slide
}

pub fn default_animation_speed() -> f32 {
    0.5
}

pub fn default_margin() -> f32 {
    16.0
}

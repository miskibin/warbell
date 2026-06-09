//! On-screen subtitles for spoken lines. A single bottom-centre caption shows what a nearby
//! character just said (villagers today; the [`Subtitles`] resource is public so the hero/ork
//! voices can push to it later). The line fades out on its own — set it and forget it.
//!
//! Speakers write the text + a display duration via [`Subtitles::say`]; the UI system below mirrors
//! the resource into one persistent `Text` node, fading the last stretch out.

use bevy::prelude::*;

use crate::ui::{label, UiFonts};

/// The currently-shown caption. A speaker calls [`Subtitles::say`]; the latest line wins.
#[derive(Resource, Default)]
pub struct Subtitles {
    text: String,
    /// `elapsed_secs` at which the caption should be gone.
    until: f32,
}

impl Subtitles {
    /// Show `text` for `dur` seconds (from `now = time.elapsed_secs()`).
    pub fn say(&mut self, now: f32, text: &str, dur: f32) {
        self.say_as(now, None, text, dur);
    }
    /// Show `text` attributed to `speaker` (renders "Name: text"); `None` = no prefix (the hero).
    pub fn say_as(&mut self, now: f32, speaker: Option<&str>, text: &str, dur: f32) {
        self.text = match speaker {
            Some(name) => format!("{name}: {text}"),
            None => text.to_string(),
        };
        self.until = now + dur;
    }
}

/// A sensible on-screen reading time for a caption: a base plus ~14 chars/sec, clamped so a
/// one-word quip still lingers and a long story doesn't overstay.
pub fn read_secs(text: &str) -> f32 {
    (1.6 + text.chars().count() as f32 / 14.0).clamp(2.5, 8.0)
}

#[derive(Component)]
struct SubtitleText;

pub struct SubtitlePlugin;

impl Plugin for SubtitlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Subtitles>()
            .add_systems(Startup, setup)
            .add_systems(Update, update);
    }
}

fn setup(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Percent(16.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(40),
        ))
        .with_children(|p| {
            p.spawn((
                label(&fonts.semibold, "", 19.0, Color::NONE),
                Node { max_width: Val::Px(760.0), ..default() },
                TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) },
                SubtitleText,
            ));
        });
}

fn update(
    time: Res<Time>,
    subs: Res<Subtitles>,
    mut q: Query<(&mut Text, &mut TextColor), With<SubtitleText>>,
) {
    let Ok((mut text, mut color)) = q.single_mut() else { return };
    let remaining = subs.until - time.elapsed_secs();
    if remaining <= 0.0 {
        if !text.0.is_empty() {
            *text = Text::new("");
        }
        color.0 = Color::NONE;
        return;
    }
    if text.0 != subs.text {
        *text = Text::new(subs.text.clone());
    }
    // Fade the final 0.6s out so lines don't pop off.
    let a = (remaining / 0.6).min(1.0);
    color.0 = Color::srgba(0.95, 0.97, 1.0, a);
}

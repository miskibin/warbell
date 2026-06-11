//! **Notice queue** — transient top-centre system messages (quality changes, events, hints), ported
//! from the original `Notice.tsx`. Any system pushes via [`Notice::push`]; each message shows for
//! ~3.5s in a gold-bordered pill, newest on top.

use bevy::prelude::*;
use std::collections::VecDeque;

use super::fonts::{label, UiFonts};
use super::theme::*;
use super::widgets::border;

const LIFETIME: f64 = 3.5;

/// Pending/active notices: `(text, born_secs)`.
#[derive(Resource, Default)]
pub struct Notice(VecDeque<(String, f64)>);

impl Notice {
    /// Queue a message (it appears next frame). `now` is `time.elapsed_secs_f64()`.
    pub fn push(&mut self, text: impl Into<String>, now: f64) {
        self.0.push_front((text.into(), now));
    }
}

#[derive(Component)]
struct NoticeRoot;
#[derive(Component)]
struct NoticeRow;

pub struct NoticePlugin;
impl Plugin for NoticePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Notice>()
            .add_systems(Startup, setup_notice)
            .add_systems(Update, update_notice);
    }
}

fn setup_notice(mut commands: Commands) {
    commands.spawn((
        NoticeRoot,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(64.0),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-140.0)),
            width: Val::Px(280.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(6.0),
            ..default()
        },
        GlobalZIndex(80),
        bevy::ui::FocusPolicy::Pass,
    ));
}

fn update_notice(
    time: Res<Time>,
    fonts: Res<UiFonts>,
    mut notice: ResMut<Notice>,
    mut commands: Commands,
    root_q: Query<Entity, With<NoticeRoot>>,
    rows_q: Query<Entity, With<NoticeRow>>,
) {
    let now = time.elapsed_secs_f64();
    notice.0.retain(|(_, born)| now - *born < LIFETIME);
    for e in &rows_q {
        commands.entity(e).try_despawn();
    }
    let Ok(root) = root_q.single() else { return };
    commands.entity(root).with_children(|col| {
        for (text, _) in notice.0.iter() {
            col.spawn((
                NoticeRow,
                Node {
                    padding: UiRect::axes(Val::Px(20.0), Val::Px(9.0)),
                    border: border(1.0),
                    border_radius: radius(R_CARD),
                    ..default()
                },
                BackgroundColor(rgba(26, 21, 15, 0.88)),
                BorderColor::all(rgba(224, 164, 88, 0.5)),
                shadow_card(),
            ))
            .with_children(|p| {
                p.spawn(label(&fonts.bold, text.clone(), 13.0, rgb(243, 230, 200)));
            });
        }
    });
}

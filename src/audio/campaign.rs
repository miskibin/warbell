//! Contextual opening VO. One current situation, no queue of obsolete instructions.
use bevy::prelude::*;
use tileworld_core::campaign::{Campaign, Objective};
use crate::quest::CampaignRes;
use crate::siege::{GamePhase, Siege};
use super::{Concept, Speak};
use super::director::VoiceManager;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CampaignLine {
    RescuePlan, HomeWorkers, FarmWorking, TorchRaid, ShamanPlan, RitualBroken,
    RallyReminder, RescueReminder, HomeReminder, FarmReminder,
    UpgradeReminder, ShamanReminder, BellReminder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Situation {
    night: usize, objective: Objective, returned: bool, farm: bool, ritual: bool,
}
impl Situation {
    fn current(c: &Campaign, siege: &Siege) -> Self {
        let night = crate::quest::lesson_night(siege);
        Self { night, objective: c.objective(night, siege.phase == GamePhase::Wave),
            returned: c.returned, farm: c.farm_worked, ritual: c.shaman_cleared }
    }
}

#[derive(Resource)]
pub(crate) struct CampaignVoice {
    previous: Option<Situation>,
    pending: Option<(CampaignLine, f32)>,
    dwell: f32,
    since_spoken: f32,
    reminders: u8,
    attempted: Option<CampaignLine>,
}

const REMINDER_DELAY: f32 = 55.0;
const REMINDER_GAP: f32 = 90.0;
const EVENT_EXPIRY: f32 = 40.0;

impl Default for CampaignVoice {
    fn default() -> Self {
        Self { previous: None, pending: None, dwell: 0.0, since_spoken: REMINDER_GAP,
            reminders: 0, attempted: None }
    }
}

fn reminder(objective: Objective) -> Option<CampaignLine> {
    use CampaignLine::*;
    Some(match objective {
        Objective::Rally => RallyReminder,
        Objective::Rescue => RescueReminder,
        Objective::ReturnHome => HomeReminder,
        Objective::FeedPeople => FarmReminder,
        Objective::PrepareDefense => UpgradeReminder,
        Objective::DisruptRitual => ShamanReminder,
        Objective::CallNight | Objective::DefendFarm | Objective::SurviveRitual => BellReminder,
        _ => return None,
    })
}

fn event(previous: Option<Situation>, now: Situation) -> Option<CampaignLine> {
    use CampaignLine::*;
    if let Some(old) = previous {
        // Only the most recent payoff survives a fast sequence of actions.
        if !old.ritual && now.ritual && now.night == 2 { return Some(RitualBroken); }
        if !old.farm && now.farm && now.night == 0 { return Some(FarmWorking); }
        if !old.returned && now.returned && !now.farm && now.night == 0 { return Some(HomeWorkers); }
        if old.night == now.night { return None; }
    }
    match now.night {
        0 if matches!(now.objective, Objective::Rally | Objective::Rescue) => Some(RescuePlan),
        1 => Some(TorchRaid),
        2 if now.objective == Objective::DisruptRitual => Some(ShamanPlan),
        _ => None,
    }
}

fn relevant(line: CampaignLine, s: Situation) -> bool {
    use CampaignLine::*;
    match line {
        RescuePlan => s.night == 0 && matches!(s.objective, Objective::Rally | Objective::Rescue),
        HomeWorkers => s.night == 0 && s.objective == Objective::FeedPeople,
        FarmWorking => s.night == 0 && s.objective == Objective::CallNight,
        TorchRaid => s.night == 1,
        ShamanPlan => s.night == 2 && s.objective == Objective::DisruptRitual,
        RitualBroken => s.night == 2 && s.ritual,
        _ => reminder(s.objective) == Some(line),
    }
}

pub(crate) fn reset(mut voice: ResMut<CampaignVoice>) { *voice = CampaignVoice::default(); }

/// Continue sets a baseline rather than replaying briefings/rewards from the abandoned run.
pub(crate) fn on_load(mut loaded: MessageReader<crate::savegame::GameLoaded>,
    campaign: Res<CampaignRes>, siege: Res<Siege>, mut voice: ResMut<CampaignVoice>,
    mut mgr: ResMut<VoiceManager>, mut cd: ResMut<super::HeroLineCooldown>,
    mut offered: ResMut<super::director::OfferedReply>,
    sinks: Query<Entity, With<super::director::VoiceSink>>, mut commands: Commands,
    time: Res<Time>, mut subtitles: ResMut<crate::subtitles::Subtitles>) {
    if loaded.read().last().is_some() {
        for entity in &sinks { commands.entity(entity).try_despawn(); }
        mgr.reset();
        *cd = default();
        offered.0 = None;
        subtitles.say(time.elapsed_secs(), "", 0.0);
        *voice = CampaignVoice { previous: Some(Situation::current(&campaign.0, &siege)),
            ..default() };
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn detect(time: Res<Time>, campaign: Res<CampaignRes>, siege: Res<Siege>,
    threat: Res<super::HeroThreat>, hero: Res<crate::player::HeroState>,
    mut mgr: ResMut<VoiceManager>, cd: Res<super::HeroLineCooldown>,
    town: Res<crate::town::TownRes>, player: Res<crate::player::PlayerRes>,
    mut voice: ResMut<CampaignVoice>, mut speak: MessageWriter<Speak>,
    // Reflex grunts/death cries share VoiceSink, but never carry a campaign instruction.
    sinks: Query<(Entity, &super::director::VoiceSink), Without<super::voice::HeroVoiceTag>>, mut commands: Commands,
    mut subtitles: ResMut<crate::subtitles::Subtitles>) {
    let now = time.elapsed_secs();
    let current = Situation::current(&campaign.0, &siege);
    let stale = mgr.active.get(&super::Speaker::Hero).is_some_and(|a|
        super::lines::LINES.iter().find(|line| line.id == a.id).is_some_and(|line|
            matches!(line.concept, Concept::Campaign(context) if
                siege.phase != GamePhase::Prep || threat.in_danger || !hero.alive || !relevant(context, current))));
    if stale {
        for (entity, sink) in &sinks {
            if sink.0 == super::Speaker::Hero { commands.entity(entity).try_despawn(); }
        }
        mgr.active.remove(&super::Speaker::Hero);
        subtitles.say(now, "", 0.0);
    }
    // Confirm actual playback, not a request which the director might reject this frame.
    if let Some(line) = voice.attempted.take() {
        if mgr.active.values().any(|a| super::lines::candidates(Concept::Campaign(line)).any(|l| l.id == a.id)
            && mgr.last_played.get(a.id).is_some_and(|at| now - *at < 1.0)) {
            voice.pending = None;
            voice.since_spoken = 0.0;
            if matches!(line, CampaignLine::RallyReminder | CampaignLine::RescueReminder |
                CampaignLine::HomeReminder | CampaignLine::FarmReminder | CampaignLine::UpgradeReminder |
                CampaignLine::ShamanReminder | CampaignLine::BellReminder) { voice.reminders += 1; }
        }
    }
    if !campaign.0.guided || siege.phase != GamePhase::Prep || !hero.alive {
        voice.pending = None;
        voice.previous = Some(Situation::current(&campaign.0, &siege));
        return;
    }
    let situation = Situation::current(&campaign.0, &siege);
    if situation.night >= 3 { voice.pending = None; return; }
    let changed = voice.previous != Some(situation);
    if changed {
        voice.pending = event(voice.previous, situation).map(|line| (line, now));
        voice.previous = Some(situation);
        voice.dwell = 0.0;
        voice.reminders = 0;
    }
    // Only unpaused, safe preparation time counts toward nudges.
    if threat.in_danger { return; }
    let dt = time.delta_secs();
    voice.dwell += dt;
    voice.since_spoken += dt;
    if voice.pending.is_some_and(|(line, at)| !relevant(line, situation) || now - at > EVENT_EXPIRY) {
        voice.pending = None;
    }
    if mgr.hero_speaking(now) || mgr.others_speaking(now) || now < cd.until { return; }
    let line = if let Some((line, _)) = voice.pending { Some(line) }
        else if voice.dwell >= REMINDER_DELAY && voice.since_spoken >= REMINDER_GAP && voice.reminders < 2 {
            reminder(situation.objective)
        } else { None };
    let Some(line) = line else { return };
    // Don't claim "build/buy now" while already waiting for a worker or unable to afford it.
    if line == CampaignLine::FarmReminder && town.0.plots.iter().any(|p|
        p.kind == Some(tileworld_core::town_store::BuildKind::Farm) && !p.is_buildable()) { return; }
    if line == CampaignLine::UpgradeReminder && player.0.gold < 40 { return; }
    speak.write(Speak::new(Concept::Campaign(line)));
    voice.attempted = Some(line);
}

/// Generic advice otherwise contradicts the rescue-first sequence (e.g. farms while marching).
pub(crate) fn replaces(concept: Concept, guided: bool, night: usize) -> bool {
    guided && night < 3 && matches!(concept, Concept::Intro | Concept::Home | Concept::AdviseFarm |
        Concept::AdviseHouses | Concept::AdviseWood | Concept::AdviseStone | Concept::AdviseUpgrade |
        Concept::AdviseWalls | Concept::AdviseBell | Concept::PrepNudge | Concept::TownThriving |
        Concept::BuildRaised | Concept::UpgradeBought | Concept::QuestDone)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(night: usize, objective: Objective) -> Situation {
        Situation { night, objective, returned: false, farm: false, ritual: false }
    }
    #[test]
    fn latest_progress_replaces_stale_instructions() {
        let old = s(0, Objective::ReturnHome);
        let current = Situation { returned: true, farm: true, ..s(0, Objective::CallNight) };
        assert_eq!(event(Some(old), current), Some(CampaignLine::FarmWorking));
        assert!(!relevant(CampaignLine::HomeReminder, current));
        assert!(!relevant(CampaignLine::HomeWorkers, current));
        assert!(!relevant(CampaignLine::RescuePlan, current));
    }
    #[test]
    fn skip_and_ranged_choices_do_not_claim_a_broken_ritual() {
        let current = s(2, Objective::SurviveRitual);
        assert_eq!(event(Some(s(1, Objective::DefendFarm)), current), None);
        assert!(!relevant(CampaignLine::RitualBroken, current));
        assert_eq!(reminder(Objective::HoldKeep), None);
        assert_eq!(reminder(Objective::FreePlay), None);
    }
    #[test]
    fn opening_replaces_conflicting_advice_but_retains_combat_and_rescue() {
        assert!(replaces(Concept::AdviseFarm, true, 0));
        assert!(replaces(Concept::Intro, true, 2));
        assert!(!replaces(Concept::LowHp, true, 0));
        assert!(!replaces(Concept::FirstRescue, true, 0));
        assert!(!replaces(Concept::AdviseFarm, true, 3));
        assert!(!replaces(Concept::AdviseFarm, false, 0));
    }
    fn app() -> App {
        let mut app = super::super::director::tests::app();
        app.init_resource::<CampaignVoice>()
            .insert_resource(crate::player::HeroState { alive: true, ..default() })
            .insert_resource(crate::town::TownRes(tileworld_core::town_store::Town::new(4, 4)))
            .insert_resource(crate::player::PlayerRes(default()))
            .add_systems(Update, detect.before(super::super::director::speak_director));
        let current = Situation::current(&app.world().resource::<CampaignRes>().0,
            app.world().resource::<Siege>());
        app.world_mut().resource_mut::<CampaignVoice>().previous = Some(current);
        app
    }
    #[test]
    fn safe_idle_time_paces_and_limits_reminders() {
        use super::super::director::tests::step;
        let mut app = app();
        app.world_mut().resource_mut::<super::super::HeroThreat>().in_danger = true;
        step(&mut app, 100.0);
        assert!(app.world().resource::<VoiceManager>().last_played.is_empty());
        app.world_mut().resource_mut::<super::super::HeroThreat>().in_danger = false;
        step(&mut app, 54.0);
        assert!(app.world().resource::<VoiceManager>().last_played.is_empty());
        step(&mut app, 1.0);
        assert_eq!(app.world().resource::<VoiceManager>().active[&super::super::Speaker::Hero].id,
            "campaign_rally_reminder");
        step(&mut app, 0.1); // Confirm playback, rather than spending a rejected request.
        assert_eq!(app.world().resource::<CampaignVoice>().reminders, 1);
        step(&mut app, 89.0);
        assert_eq!(app.world().resource::<CampaignVoice>().reminders, 1);
        step(&mut app, 1.1);
        step(&mut app, 0.1);
        assert_eq!(app.world().resource::<CampaignVoice>().reminders, 2);
        let last = app.world().resource::<VoiceManager>().last_played["campaign_rally_reminder"];
        step(&mut app, 200.0);
        assert_eq!(app.world().resource::<VoiceManager>().last_played["campaign_rally_reminder"], last);
    }
    #[test]
    fn progress_and_battle_cancel_live_stale_instructions() {
        use super::super::director::tests::step;
        let mut app = app();
        step(&mut app, 55.0);
        app.world_mut().resource_mut::<CampaignRes>().0.rescued = true;
        step(&mut app, 0.1);
        assert!(app.world().resource::<VoiceManager>().active.is_empty());
        assert_eq!(app.world_mut().query::<&super::super::director::VoiceSink>().iter(app.world()).count(), 0);
        assert_eq!(app.world().resource::<CampaignVoice>().reminders, 0);
        // A later safe reminder must also stop the moment combat starts.
        step(&mut app, 100.0);
        assert!(app.world().resource::<VoiceManager>().hero_speaking(155.1));
        app.world_mut().resource_mut::<super::super::HeroThreat>().in_danger = true;
        step(&mut app, 0.1);
        assert!(app.world().resource::<VoiceManager>().active.is_empty());
    }

}

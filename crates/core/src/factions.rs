//! Port of src/world/factions.ts — faction taxonomy + hostility predicates.

/// Which warband an ork belongs to. Different factions are mutually hostile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrkFaction {
    Red,
    Blue,
}

/// Behaviour class for a wild animal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimalFaction {
    Predator,
    Prey,
    Boar,
}

/// Two orks fight when they belong to opposing camps.
pub fn orks_hostile(a: OrkFaction, b: OrkFaction) -> bool {
    a != b
}

/// A predator hunts prey (deer/rabbit). Predators don't hunt each other.
pub fn preys_on(hunter: AnimalFaction, target: AnimalFaction) -> bool {
    hunter == AnimalFaction::Predator && target == AnimalFaction::Prey
}

/// Prey flee anything that would harm them: predators and (enraged) boars.
pub fn threatens_prey(f: AnimalFaction) -> bool {
    matches!(f, AnimalFaction::Predator | AnimalFaction::Boar)
}

#[cfg(test)]
mod tests {
    // Port of src/world/factions.test.ts — lock the targeting truth tables.
    use super::AnimalFaction::*;
    use super::OrkFaction::*;
    use super::*;

    #[test]
    fn opposing_warbands_are_hostile() {
        assert!(orks_hostile(Red, Blue));
        assert!(orks_hostile(Blue, Red));
    }

    #[test]
    fn same_warband_is_not_hostile() {
        assert!(!orks_hostile(Red, Red));
        assert!(!orks_hostile(Blue, Blue));
    }

    #[test]
    fn predators_hunt_prey() {
        assert!(preys_on(Predator, Prey));
    }

    #[test]
    fn predators_do_not_hunt_predators_boars_or_nothing() {
        assert!(!preys_on(Predator, Predator));
        assert!(!preys_on(Predator, Boar));
    }

    #[test]
    fn prey_and_boars_are_not_hunters() {
        assert!(!preys_on(Prey, Prey));
        assert!(!preys_on(Boar, Prey));
    }

    #[test]
    fn predators_and_boars_threaten_prey() {
        assert!(threatens_prey(Predator));
        assert!(threatens_prey(Boar));
    }

    #[test]
    fn prey_does_not_threaten_prey() {
        assert!(!threatens_prey(Prey));
    }
}

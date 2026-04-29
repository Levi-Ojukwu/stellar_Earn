use crate::errors::Error;
use crate::events;
use crate::storage;
use crate::types::{Badge, BadgeType, Role, UserBadges, UserCore};
use soroban_sdk::{Address, Env, String, Symbol, Vec};

const LEVEL_2_XP: u64 = 300;
const LEVEL_3_XP: u64 = 600;
const LEVEL_4_XP: u64 = 1000;
const LEVEL_5_XP: u64 = 1500;

/// Award XP to a user upon quest completion.
/// Only reads/writes UserCore (hot path) — badge Vec not touched.
pub fn award_xp(env: &Env, user: &Address, xp_amount: u64) -> Result<UserCore, Error> {
    let mut stats = storage::get_user_stats_or_default(env, user);

    stats.xp += xp_amount;
    stats.quests_completed += 1;

    let new_level = calculate_level(stats.xp);
    let level_up = new_level > stats.level;
    stats.level = new_level;

    storage::set_user_stats(env, user, &stats);

    events::xp_awarded(env, user.clone(), xp_amount, stats.xp, stats.level);

    if level_up {
        events::level_up(env, user.clone(), stats.level);
    }

    Ok(stats)
}

/// Calculate user level based on total XP
pub fn calculate_level(xp: u64) -> u32 {
    if xp >= LEVEL_5_XP {
        5
    } else if xp >= LEVEL_4_XP {
        4
    } else if xp >= LEVEL_3_XP {
        3
    } else if xp >= LEVEL_2_XP {
        2
    } else {
        1
    }
}

fn require_badge_admin(env: &Env, caller: &Address) -> Result<(), Error> {
    if !(storage::is_super_admin(env, caller)
        || storage::has_role(env, caller, &Role::Admin)
        || storage::has_role(env, caller, &Role::BadgeAdmin))
    {
        return Err(Error::Unauthorized);
    }
    Ok(())
}

/// Register a new badge type in the on-chain registry (admin-authorized).
pub fn register_badge_type(
    env: &Env,
    caller: &Address,
    badge_type: BadgeType,
) -> Result<(), Error> {
    caller.require_auth();
    require_badge_admin(env, caller)?;

    if storage::has_badge_type(env, &badge_type.id) {
        return Err(Error::BadgeTypeAlreadyExists);
    }

    storage::set_badge_type(env, &badge_type);
    storage::add_badge_type_id(env, &badge_type.id);
    events::badge_type_registered(env, badge_type.id.clone(), badge_type.name.clone());
    Ok(())
}

/// Update an existing badge type definition (admin-authorized).
pub fn update_badge_type(
    env: &Env,
    caller: &Address,
    badge_type: BadgeType,
) -> Result<(), Error> {
    caller.require_auth();
    require_badge_admin(env, caller)?;

    if !storage::has_badge_type(env, &badge_type.id) {
        return Err(Error::BadgeTypeNotFound);
    }

    storage::set_badge_type(env, &badge_type);
    events::badge_type_updated(env, badge_type.id.clone());
    Ok(())
}

/// Remove a badge type from the registry (admin-authorized).
/// Existing user grants are not retroactively revoked — they remain in
/// `UserBadges` Vecs and clients can decide how to display orphan ids.
pub fn remove_badge_type(env: &Env, caller: &Address, id: Symbol) -> Result<(), Error> {
    caller.require_auth();
    require_badge_admin(env, caller)?;

    if !storage::has_badge_type(env, &id) {
        return Err(Error::BadgeTypeNotFound);
    }

    storage::remove_badge_type(env, &id);
    storage::remove_badge_type_id(env, &id);
    events::badge_type_removed(env, id);
    Ok(())
}

pub fn get_badge_type(env: &Env, id: &Symbol) -> Result<BadgeType, Error> {
    storage::get_badge_type(env, id)
}

pub fn list_badge_types(env: &Env) -> Vec<BadgeType> {
    storage::list_badge_types(env)
}

/// Seed the five legacy badge types during contract initialization.
/// Called once from `initialize` so existing flows keep working without an
/// explicit registration call from the deployer.
pub fn seed_default_badge_types(env: &Env) {
    let defaults = [
        (Badge::rookie(env).id, "Rookie", "First quest completed.", 0u64),
        (Badge::explorer(env).id, "Explorer", "Reached level 2.", LEVEL_2_XP),
        (Badge::veteran(env).id, "Veteran", "Reached level 3.", LEVEL_3_XP),
        (Badge::master(env).id, "Master", "Reached level 4.", LEVEL_4_XP),
        (Badge::legend(env).id, "Legend", "Reached level 5.", LEVEL_5_XP),
    ];

    for (id, name, desc, threshold) in defaults.iter() {
        if storage::has_badge_type(env, id) {
            continue;
        }
        let bt = BadgeType {
            id: id.clone(),
            name: String::from_str(env, name),
            description: String::from_str(env, desc),
            xp_threshold: *threshold,
            is_active: true,
        };
        storage::set_badge_type(env, &bt);
        storage::add_badge_type_id(env, id);
    }
}

/// Grant a badge to a user (admin-authorized).
/// Validates the badge id exists in the registry and is active.
/// Only reads/writes UserBadges (cold path) — XP counters not touched.
pub fn grant_badge(env: &Env, caller: &Address, user: &Address, badge: Badge) -> Result<(), Error> {
    caller.require_auth();
    require_badge_admin(env, caller)?;

    let bt = storage::get_badge_type(env, &badge.id)?;
    if !bt.is_active {
        return Err(Error::BadgeTypeInactive);
    }

    let mut user_badges = storage::get_user_badges(env, user);

    if !user_badges.badges.contains(&badge) {
        user_badges.badges.push_back(badge.clone());
        storage::set_user_badges(env, user, &user_badges);
        events::badge_granted(env, user.clone(), badge);
    }

    Ok(())
}

/// Get user reputation stats (UserCore only — no badge Vec).
pub fn get_user_stats(env: &Env, user: &Address) -> UserCore {
    storage::get_user_stats_or_default(env, user)
}

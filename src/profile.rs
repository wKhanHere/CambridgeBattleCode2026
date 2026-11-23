use std::cell::Cell;

use cambc::{Result, Team};

#[derive(Clone, Copy)]
pub struct Profile {
    pub orient: u8,
    pub draft: u8,
    pub flags: u8,
}

pub const AMBUSH: u8 = 1;
pub const BIG_BAGS: u8 = 2;
pub const TWINS: u8 = 4;
pub const PACK: u8 = 16;
pub const PRISM_CARRY: u8 = 128;

thread_local! {
    static CACHE: Cell<Option<Profile>> = const { Cell::new(None) };
}

pub fn get() -> Profile {
    CACHE.with(|c| {
        let p = c.get().unwrap_or_else(|| {
            lookup().unwrap_or(Profile {
                orient: 0,
                draft: 0,
                flags: AMBUSH,
            })
        });
        c.set(Some(p));
        p
    })
}

pub fn has(flag: u8) -> bool {
    get().flags & flag != 0
}

pub fn enable(flag: u8) {
    CACHE.with(|c| {
        let mut p = get();
        p.flags |= flag;
        c.set(Some(p));
    })
}

fn lookup() -> Result<Profile> {
    let corner = |team| -> Result<Option<(i32, i32)>> {
        Ok(cambc::map::spawn_region(team)?
            .positions()?
            .iter()
            .map(|p| (p.x, p.y))
            .min())
    };
    let first = corner(Team::Ally)? < corner(Team::Enemy)?;
    let key = (
        cambc::map::width()?,
        cambc::map::height()?,
        cambc::map::camp_groups()?.len(),
        first,
    );
    let (orient, draft, flags) = match key {
        (100, 100, 12, true) => (0, 0, AMBUSH),
        (100, 100, 12, false) => (0, 0, AMBUSH),
        (41, 41, 20, true) => (6, 0, AMBUSH),
        (41, 41, 20, false) => (0, 0, AMBUSH),
        (50, 50, 14, true) => (0, 0, AMBUSH),
        (50, 50, 14, false) => (0, 0, AMBUSH),
        (71, 71, 30, true) => (1, 1, AMBUSH),
        (71, 71, 30, false) => (0, 1, AMBUSH),
        (41, 30, 8, true) => (1, 0, AMBUSH),
        (41, 30, 8, false) => (1, 0, AMBUSH),
        (30, 30, 10, true) => (0, 0, AMBUSH),
        (30, 30, 10, false) => (3, 0, AMBUSH),
        (80, 80, 24, true) => (0, 2, AMBUSH),
        (80, 80, 24, false) => (0, 2, AMBUSH),
        (30, 30, 6, true) => (0, 0, AMBUSH),
        (30, 30, 6, false) => (0, 0, AMBUSH),
        (40, 40, 14, true) => (0, 0, AMBUSH),
        (40, 40, 14, false) => (0, 0, AMBUSH),
        (90, 30, 42, true) => (0, 0, AMBUSH),
        (90, 30, 42, false) => (0, 0, AMBUSH),
        (50, 50, 20, true) => (0, 0, AMBUSH),
        (50, 50, 20, false) => (0, 0, AMBUSH),
        (100, 100, 24, false) => (0, 2, AMBUSH),
        (100, 100, 24, true) => (0, 2, AMBUSH),
        (50, 41, 31, true) => (0, 3, AMBUSH),
        (50, 41, 31, false) => (0, 3, AMBUSH),
        (50, 41, 32, true) => (0, 3, AMBUSH),
        (50, 41, 32, false) => (0, 3, AMBUSH),
        (40, 40, 22, false) => (1, 0, AMBUSH),
        (40, 40, 22, true) => (0, 0, AMBUSH),
        (61, 61, 11, true) => (1, 0, AMBUSH),
        (61, 61, 11, false) => (1, 0, AMBUSH),
        (51, 40, 12, true) => (0, 3, AMBUSH),
        (51, 40, 12, false) => (0, 3, AMBUSH),
        (50, 50, 29, true) => (0, 0, AMBUSH),
        (50, 50, 29, false) => (0, 0, AMBUSH),
        (61, 71, 28, true) => (0, 0, AMBUSH),
        (61, 71, 28, false) => (0, 0, AMBUSH),
        (31, 31, 15, true) => (0, 0, AMBUSH),
        (31, 31, 15, false) => (3, 0, AMBUSH),
        (31, 31, 12, true) => (0, 0, AMBUSH),
        (31, 31, 12, false) => (0, 0, AMBUSH),
        (60, 60, 46, true) => (0, 0, AMBUSH),
        (60, 60, 46, false) => (0, 0, AMBUSH),
        (35, 31, _, _) => (0, 4, AMBUSH | PRISM_CARRY),
        (45, 45, 16..=26, _) => (0, 1, AMBUSH),
        (51, 31, _, _) => (0, 5, AMBUSH),
        (51, 51, 1..=16, _) => (0, 1, AMBUSH),
        (41, 61, _, _) => (0, 6, AMBUSH),
        (89, 71, _, _) => (0, 2, AMBUSH),
        (50, 46, _, _) => (0, 7, AMBUSH),
        _ => (0, 0, AMBUSH),
    };
    Ok(Profile {
        orient,
        draft,
        flags,
    })
}

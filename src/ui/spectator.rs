use super::{Painter, Rect, WHITE};
use crate::net_presentation::State as Roster;
use std::time::Instant;

#[derive(Default, Clone)]
enum Group {
    #[default]
    Root,
    Players(Option<String>),
    Teams,
}
#[derive(Clone)]
enum Target {
    Group(Group),
    Player([u8; 16]),
    Page(i8),
    Close,
}
struct Entry {
    name: String,
    target: Target,
    enabled: bool,
}
#[derive(Default)]
pub struct Menu {
    pub spectator: bool,
    opened: Option<Instant>,
    group: Group,
    page: usize,
    selected: Option<usize>,
}
impl Menu {
    pub fn set_spectator(&mut self, spectator: bool) {
        if self.spectator != spectator {
            *self = Self { spectator, ..Self::default() };
        }
    }
    pub fn active(&self) -> bool {
        self.spectator && self.opened.is_some_and(|t| t.elapsed().as_secs_f32() < 5.)
    }
    fn entries(&self, roster: &Roster) -> Vec<Entry> {
        let players = |team: Option<&String>| {
            let mut players = roster
                .players
                .iter()
                .filter(|(_, p)| {
                    p.mode != 3
                        && team.is_none_or(|team| roster.memberships.get(&p.name) == Some(team))
                })
                .collect::<Vec<_>>();
            players.sort_by_key(|(id, _)| {
                (
                    i64::from_be_bytes(id[..8].try_into().unwrap()),
                    i64::from_be_bytes(id[8..].try_into().unwrap()),
                )
            });
            players
                .into_iter()
                .map(|(id, p)| Entry {
                    name: p.name.clone(),
                    target: Target::Player(*id),
                    enabled: true,
                })
                .collect::<Vec<_>>()
        };
        match &self.group {
            Group::Players(team) => players(team.as_ref()),
            Group::Teams => {
                let mut teams = roster.teams.iter().collect::<Vec<_>>();
                teams.sort_by_key(|(key, _)| *key);
                teams
                    .into_iter()
                    .map(|(key, team)| Entry {
                        name: team.label.clone(),
                        target: Target::Group(Group::Players(Some(key.clone()))),
                        enabled: !players(Some(key)).is_empty(),
                    })
                    .collect()
            }
            Group::Root => vec![
                Entry {
                    name: "Teleport to player".into(),
                    target: Target::Group(Group::Players(None)),
                    enabled: !players(None).is_empty(),
                },
                Entry {
                    name: "Teleport to team member".into(),
                    target: Target::Group(Group::Teams),
                    enabled: roster.teams.keys().any(|t| !players(Some(t)).is_empty()),
                },
            ],
        }
    }
    fn slot(&self, index: usize, all: &[Entry]) -> Option<Entry> {
        let (name, target, enabled) = match index {
            8 => ("Close menu", Target::Close, true),
            7 => (
                "Next page",
                Target::Page(1),
                index + self.page * 6 < all.len(),
            ),
            0 if self.page > 0 => ("Previous page", Target::Page(-1), true),
            _ => {
                let e = all.get(index + self.page * 6)?;
                return Some(Entry {
                    name: e.name.clone(),
                    target: e.target.clone(),
                    enabled: e.enabled,
                });
            }
        };
        Some(Entry {
            name: name.into(),
            target,
            enabled,
        })
    }
    pub fn activate(&mut self, key: Option<usize>, roster: &Roster) -> Option<[u8; 16]> {
        if !self.active() {
            self.group = Group::Root;
            self.page = 0;
            self.selected = None;
            self.opened = Some(Instant::now());
            return None;
        }
        self.opened = Some(Instant::now());
        let i = key.or(self.selected)?;
        let entry = self.slot(i, &self.entries(roster))?;
        if self.selected != Some(i) || !entry.enabled {
            self.selected = Some(i);
            return None;
        }
        match entry.target {
            Target::Player(id) => Some(id),
            Target::Close => {
                self.opened = None;
                None
            }
            Target::Page(dir) => {
                self.page = self.page.saturating_add_signed(dir as isize);
                None
            }
            Target::Group(group) => {
                self.group = group;
                self.page = 0;
                self.selected = None;
                None
            }
        }
    }
    pub fn wheel(&mut self, delta: f32, roster: &Roster) {
        if !self.active() {
            return;
        }
        let entries = self.entries(roster);
        let mut i = self.selected.map_or(-1, |i| i as i32);
        let dir = if delta > 0. { -1 } else { 1 };
        loop {
            i += dir;
            if !(0..9).contains(&i) {
                return;
            }
            if self.slot(i as usize, &entries).is_some_and(|e| e.enabled) {
                self.selected = Some(i as usize);
                self.opened = Some(Instant::now());
                return;
            }
        }
    }
    pub(super) fn draw(&self, p: &mut Painter<'_>, roster: &Roster) {
        if !self.active() {
            return;
        }
        let [w, h] = p.frame.size;
        let alpha = ((5. - self.opened.unwrap().elapsed().as_secs_f32()) / 2.).clamp(0., 1.);
        let cx = (w / 2.).floor();
        let y = (h - 22. * alpha).floor();
        let x = cx - 91.;
        let white = [1., 1., 1., alpha];
        p.image(
            "textures/gui/widgets.png",
            Rect {
                x,
                y,
                w: 182.,
                h: 22.,
            },
            [0., 0., 182., 22.],
            white,
        );
        if let Some(slot) = self.selected {
            p.image(
                "textures/gui/widgets.png",
                Rect {
                    x: x - 1. + slot as f32 * 20.,
                    y: y - 1.,
                    w: 24.,
                    h: 22.,
                },
                [0., 22., 24., 22.],
                white,
            );
        }
        let entries = self.entries(roster);
        for i in 0..9 {
            if let Some(e) = self.slot(i, &entries) {
                let x = x + 3. + i as f32 * 20.;
                let gray = if e.enabled { 1. } else { 0.25 };
                let color = [gray, gray, gray, alpha];
                match &e.target {
                    Target::Player(uuid) => {
                        let skin = roster.skin_region(p.r, *uuid);
                        let face = Rect {
                            x: x + 2.,
                            y: y + 5.,
                            w: 12.,
                            h: 12.,
                        };
                        p.quad(face, skin.subrect_uv(8., 8., 8., 8.), skin.layer, color);
                        p.quad(face, skin.subrect_uv(40., 8., 8., 8.), skin.layer, color);
                    }
                    Target::Group(Group::Players(Some(team))) => {
                        let uuid = roster
                            .players
                            .iter()
                            .find(|(_, pl)| {
                                pl.mode != 3 && roster.memberships.get(&pl.name) == Some(team)
                            })
                            .map(|(&id, _)| id);
                        let skin = uuid.map_or_else(
                            || p.r.skins.resolve(p.r, None, false),
                            |id| roster.skin_region(p.r, id),
                        );
                        let face = Rect {
                            x: x + 2.,
                            y: y + 5.,
                            w: 12.,
                            h: 12.,
                        };
                        p.quad(face, skin.subrect_uv(8., 8., 8., 8.), skin.layer, color);
                        p.quad(face, skin.subrect_uv(40., 8., 8., 8.), skin.layer, color);
                    }
                    _ => {
                        let u = match e.target {
                            Target::Close => 128.,
                            Target::Page(-1) => 144.,
                            Target::Page(_) => 160.,
                            Target::Group(Group::Teams) => 16.,
                            _ => 0.,
                        };
                        p.image(
                            "textures/gui/spectator_widgets.png",
                            Rect {
                                x,
                                y: y + 3.,
                                w: 16.,
                                h: 16.,
                            },
                            [u, 0., 16., 16.],
                            color,
                        );
                    }
                }
                if e.enabled {
                    let key = crate::keybindings::Binding::ALL
                        .iter()
                        .find(|b| b.hotbar() == Some(i));
                    if let Some(key) = key {
                        let label = crate::keybindings::key_label(
                            p.s.bindings.code(*key),
                            &p.tr("key.mouseButton"),
                        );
                        p.text(x + 17. - p.width(&label), y + 12., &label, white);
                    }
                }
            }
        }
        let name = self
            .selected
            .and_then(|i| self.slot(i, &entries))
            .map(|e| e.name)
            .unwrap_or_else(|| {
                match self.group {
                    Group::Root => "Press a key to select a command, and again to use it.",
                    Group::Players(_) => "Select a player to teleport to",
                    Group::Teams => "Select a team to teleport to",
                }
                .into()
            });
        p.center(cx, h - 35., &name, [WHITE[0], WHITE[1], WHITE[2], alpha]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net_presentation::PlayerEntry;
    #[test]
    fn selecting_twice_emits_target_uuid_only_for_non_spectators() {
        let mut menu=Menu{spectator:true,..Default::default()};let mut roster=Roster::default();
        let first=[255;16];let second=[1;16];let spectator=[0;16];
        for (uuid,name,mode) in [(first,"First",1),(second,"Second",0),(spectator,"Camera",3)]{
            roster.players.insert(uuid,PlayerEntry{name:name.into(),mode,..Default::default()});
        }
        for action in [None,Some(0),Some(0),Some(0)] {assert_eq!(menu.activate(action,&roster),None);}
        assert_eq!(menu.activate(None,&roster),Some(first));
        menu.wheel(-1.,&roster);assert_eq!(menu.activate(None,&roster),Some(second));
        assert_eq!(menu.activate(Some(8),&roster),None);assert_eq!(menu.activate(None,&roster),None);
        assert!(!menu.active());
    }
}

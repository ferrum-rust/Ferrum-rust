use super::*;
use glam::IVec3;
#[derive(Default)]
pub(crate) struct Editor {
    pub position: IVec3,
    pub command: text_field::TextField,
    pub track: bool,
    pub output: String,
}
impl Editor {
    pub(crate) fn from_world(
        world: &crate::world::World,
        position: IVec3,
        resources: &Resources,
        settings: &Settings,
    ) -> Self {
        let value = crate::gameplay::command_blocks::read(world, position);
        let output = value
            .output
            .map(|s| {
                chat::components::parse(&s, |key| resources.tr(&settings.language, key))
                    .into_iter()
                    .map(|run| run.text)
                    .collect()
            })
            .unwrap_or_else(|| "-".into());
        Self {
            position,
            command: text_field::TextField::with_limit(&value.command, 32767),
            track: value.track,
            output,
        }
    }
}
pub(super) fn draw(p: &mut Painter<'_>, edit: &Editor, time: f32) {
    let [w, h] = p.frame.size;
    let cx = (w / 2.).floor();
    let gray = [160. / 255., 160. / 255., 160. / 255., 1.];
    p.rect(0., 0., w, h, [0., 0., 0., 0.6]);
    p.center(cx, 20., &p.tr("advMode.setCommand"), WHITE);
    p.text(cx - 150., 37., &p.tr("advMode.command"), gray);
    edit.command.paint(
        p,
        Rect {
            x: cx - 150.,
            y: 50.,
            w: 300.,
            h: 20.,
        },
        true,
        time,
        32767,
        true,
        Action::CommandCursor,
    );
    for (i, key) in [
        "advMode.nearestPlayer",
        "advMode.randomPlayer",
        "advMode.allPlayers",
        "advMode.allEntities",
    ]
    .iter()
    .enumerate()
    {
        p.text(cx - 150., 75. + i as f32 * 9., &p.tr(key), gray);
    }
    let output = if edit.track {
        edit.output.as_str()
    } else {
        "-"
    };
    if !output.is_empty() {
        p.text(cx - 150., 136., &p.tr("advMode.previousOutput"), gray);
        text_field::TextField::new(output).paint(
            p,
            Rect {
                x: cx - 150.,
                y: 150.,
                w: 276.,
                h: 20.,
            },
            false,
            time,
            32767,
            true,
            |_| Action::CommandOutput,
        );
    }
    p.button(
        Rect {
            x: cx + 130.,
            y: 150.,
            w: 20.,
            h: 20.,
        },
        if edit.track { "O" } else { "X" },
        Action::ToggleCommandOutput,
        true,
    );
    p.button(
        Rect {
            x: cx - 154.,
            y: (h / 4.).floor() + 132.,
            w: 150.,
            h: 20.,
        },
        &p.tr("gui.done"),
        Action::SaveCommandBlock,
        !edit.command.value.trim().is_empty(),
    );
    p.button(
        Rect {
            x: cx + 4.,
            y: (h / 4.).floor() + 132.,
            w: 150.,
            h: 20.,
        },
        &p.tr("gui.cancel"),
        Action::Resume,
        true,
    );
}

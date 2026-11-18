use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Snapshot {
    slots: Vec<Option<[i16; 3]>>,
    cursor: Option<[i16; 3]>,
}
#[derive(Deserialize)]
struct Step {
    #[serde(flatten)]
    snapshot: Snapshot,
    click: [i16; 3],
    clicked: Option<[i16; 3]>,
    txn: i16,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    kind: String,
    initial: Snapshot,
    steps: Vec<Step>,
}
fn slot(v: [i16; 3]) -> Slot {
    Slot {
        id: v[0],
        count: v[1] as u8,
        damage: v[2],
        nbt: vec![0],
    }
}
fn wire_states(remote: &Remote) -> &[Option<Slot>] {
    remote
        .extra
        .window
        .as_ref()
        .map_or(&remote.slots, |window| &window.slots)
}
fn address(r: &Remote, n: usize) -> SlotRef {
    if let Some(w) = r.extra.window.as_ref() {
        if let Some(i) = w.player_index(n) {
            return SlotRef::Player(i);
        }
        if w.screen() != Screen::Crafting {
            return SlotRef::Container(n);
        }
    } else if let Some(i) = inventory_index(n) {
        return SlotRef::Player(i);
    }
    if n == 0 {
        SlotRef::CraftOutput
    } else {
        SlotRef::Craft(n - 1)
    }
}

#[test]
fn original_c0e_returns_and_predictions_for_chest_furnace_and_crafting() {
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-remote-windows.json"
    ))
    .unwrap();
    let mut clicks = 0;
    for case in cases {
        let (client, queue) = Client::window_test_transport();
        let mut remote = Remote::with_client(client);
        remote.ready = true;
        let mut sim = Simulation::default();
        let id = if case.kind == "personal" { 0 } else { 5 };
        let kind = match case.kind.as_str() {
            "crafting" => "minecraft:crafting_table",
            "furnace" => "minecraft:furnace",
            _ => "minecraft:chest",
        };
        if id != 0 {
            remote.open_window(id, kind.into(), "test".into(), 27, &mut sim);
        }
        remote
            .window_items(
                id,
                case.initial.slots.iter().map(|v| v.map(slot)).collect(),
                &mut sim,
            )
            .unwrap();
        remote
            .set_window_slot(-1, -1, case.initial.cursor.map(slot), &mut sim)
            .unwrap();
        for (i, step) in case.steps.into_iter().enumerate() {
            let [n, button, mode] = step.click;
            let action = match mode {
                0 => ClickAction::Pickup { right: button != 0 },
                1 => ClickAction::QuickMove,
                2 => ClickAction::Hotbar(button as usize),
                4 => ClickAction::Throw { all: button != 0 },
                6 => ClickAction::Collect {
                    reverse: button != 0,
                },
                _ => panic!("unexpected oracle mode {mode}"),
            };
            let addr = address(&remote, n as usize);
            remote.window_click(addr, action, &mut sim).unwrap();
            let Command::ClickWindow {
                window_id,
                slot: native,
                button: b,
                mode: m,
                action,
                clicked,
            } = queue.try_recv().unwrap()
            else {
                panic!("C0E missing")
            };
            assert_eq!(
                (window_id, native, i16::from(b), i16::from(m), action),
                (id, n, button, mode, step.txn),
                "{} step {i}",
                case.name
            );
            assert_eq!(
                clicked,
                step.clicked.map(slot),
                "{} step {i} C0E clicked stack",
                case.name
            );
            let expected = step
                .snapshot
                .slots
                .into_iter()
                .map(|v| v.map(slot))
                .collect::<Vec<_>>();
            for (j, (native, original)) in wire_states(&remote).iter().zip(&expected).enumerate() {
                assert_eq!(
                    native, original,
                    "{} step {i} resulting slot {j}",
                    case.name
                );
            }
            assert_eq!(
                remote.cursor,
                step.snapshot.cursor.map(slot),
                "{} cursor at {i}",
                case.name
            );
            clicks += 1;
        }
    }
    assert_eq!(clicks, 45);
}

#[test]
fn window_synchronization_keeps_player_backing_slots_and_transactions() {
    let (client, queue) = Client::window_test_transport();
    let mut r = Remote::with_client(client);
    r.ready = true;
    let mut sim = Simulation::default();
    r.set_window_slot(0, 36, Some(slot([4, 2, 0])), &mut sim)
        .unwrap();
    r.window_click(
        SlotRef::Player(0),
        ClickAction::Pickup { right: false },
        &mut sim,
    )
    .unwrap();
    assert!(matches!(
        queue.try_recv(),
        Ok(Command::ClickWindow {
            window_id: 0,
            action: 1,
            ..
        })
    ));
    r.open_window(7, "minecraft:chest".into(), "Chest".into(), 27, &mut sim);
    r.set_window_slot(7, 54, Some(slot([3, 13, 0])), &mut sim)
        .unwrap();
    let identity = sim.inventory.slots[0].as_ref().unwrap().identity;
    r.set_window_slot(7, 0, Some(slot([88, 42, 0])), &mut sim)
        .unwrap();
    r.set_window_slot(-1, 19, Some(slot([1, 1, 0])), &mut sim)
        .unwrap();
    assert_eq!(sim.inventory.slots[0].as_ref().unwrap().identity, identity);
    assert_eq!(sim.inventory.cursor.as_ref().unwrap().id, 1);
    r.set_window_slot(0, 36, Some(slot([2, 8, 0])), &mut sim)
        .unwrap();
    assert_eq!(wire_states(&r)[54], Some(slot([2, 8, 0])));
    r.set_window_slot(0, 10, Some(slot([19, 1, 0])), &mut sim)
        .unwrap();
    assert_eq!(sim.inventory.slots[10], None);
    let mut personal = vec![None; 45];
    personal[10] = Some(slot([36, 4, 0]));
    personal[1] = Some(slot([17, 1, 0]));
    r.inventory_blocked = true;
    r.window_items(0, personal, &mut sim).unwrap();
    assert!(r.inventory_blocked);
    assert_eq!(wire_states(&r)[28], Some(slot([36, 4, 0])));
    r.window_items(7, vec![Some(slot([12, 1, 0]))], &mut sim)
        .unwrap();
    assert!(!r.inventory_blocked);
    r.window_click(
        SlotRef::Container(0),
        ClickAction::Pickup { right: false },
        &mut sim,
    )
    .unwrap();
    assert!(matches!(
        queue.try_recv(),
        Ok(Command::ClickWindow {
            window_id: 7,
            action: 1,
            ..
        })
    ));
    r.reset_window(&mut sim).unwrap();
    assert_eq!(sim.inventory.crafting[0].as_ref().unwrap().id, 17);
    r.window_click(
        SlotRef::Player(0),
        ClickAction::Pickup { right: false },
        &mut sim,
    )
    .unwrap();
    assert!(matches!(
        queue.try_recv(),
        Ok(Command::ClickWindow {
            window_id: 0,
            action: 2,
            ..
        })
    ));
    r.open_window(
        8,
        "minecraft:crafting_table".into(),
        "Craft".into(),
        0,
        &mut sim,
    );
    r.set_window_slot(8, 1, Some(slot([17, 3, 0])), &mut sim)
        .unwrap();
    // ContainerWorkbench recomputes its result as soon as the backing grid changes.
    assert_eq!(wire_states(&r)[0], Some(slot([5, 4, 0])));
    r.window_click(
        SlotRef::CraftOutput,
        ClickAction::Pickup { right: false },
        &mut sim,
    )
    .unwrap();
    let Command::ClickWindow { clicked, .. } = queue.try_recv().unwrap() else {
        panic!("missing C0E")
    };
    assert_eq!(clicked, Some(slot([5, 4, 0])));
}

#[test]
#[ignore="requires installed 1.8.9 textures/languages"]
fn original_special_window_actions(){
    use crate::{protocol::{Reader,Writer},ui::{self,Action,special_windows::Special},network::special_payload::Payload};
    let fixture:std::collections::HashMap<String,String>=serde_json::from_str(include_str!("../tests/fixtures/vanilla189-special-windows.json")).unwrap();
    let wire=|key:&str|{let s=&fixture[key];(0..s.len()).step_by(2).map(|i|u8::from_str_radix(&s[i..i+2],16).unwrap()).collect::<Vec<_>>()};
    let (client,queue)=Client::window_test_transport();let mut r=Remote::with_client(client);r.ready=true;
    let mut sim=Simulation::default();let mut gui=ui::Gui::default();
    let res=crate::validation::resources();let settings=crate::settings::Settings::default();let assets=crate::validation::assets();
    let screen=|gui:&ui::Gui,sim:&Simulation|ui::build(gui,&settings,res,assets,[800,600],Some(sim),&ui::Stats::default());
    r.open_window(4,"minecraft:anvil".into(),"Anvil".into(),0,&mut sim);
    r.window_items(4,vec![Some(slot([276,1,0]))],&mut sim).unwrap();
    gui.open(Screen::Chest);
    gui.container_view=r.current_window_view();
    let w=r.extra.window.as_ref().unwrap();
    assert_eq!(gui.special.sync(&w.kind,w.container.slots[0].as_ref(),&w.properties,res,"en_US"),Some(String::new()));
    assert!(screen(&gui,&sim).buttons.iter().any(|b|matches!(b.action,Action::AnvilCursor(_))));
    let Special::Anvil{field,..}=&mut gui.special else{panic!()};field.key(winit::keyboard::KeyCode::KeyA,false,true,30);field.insert("모루",30);
    r.rename_anvil(gui.special.rename().unwrap()).unwrap();
    let Command::WindowPayload(value)=queue.try_recv().unwrap()else{panic!()};let mut body=Writer::new();value.write(&mut body).unwrap();assert_eq!(body.0,wire("rename"));
    r.open_window(5,"minecraft:beacon".into(),"Beacon".into(),0,&mut sim);
    r.window_items(5,vec![Some(slot([264,1,0]))],&mut sim).unwrap();
    let w=r.extra.window.as_mut().unwrap();w.properties.insert(0,4);gui.special.sync(&w.kind,w.container.slots[0].as_ref(),&w.properties,res,"en_US");
    gui.container_view=r.current_window_view();
    for (second,power) in [(false,1),(true,10)]{
        assert!(screen(&gui,&sim).buttons.iter().any(|b|b.enabled && matches!(b.action,Action::BeaconSelect(s,p) if s==second && p==power)));
        gui.special.beacon(second,power);
    }
    assert!(screen(&gui,&sim).buttons.iter().any(|b|b.enabled && matches!(b.action,Action::BeaconDone)));
    let Special::Beacon{primary,secondary,..}=gui.special else{panic!()};assert!(r.confirm_beacon(primary,secondary).unwrap());
    let Command::WindowPayload(value)=queue.try_recv().unwrap()else{panic!()};let mut body=Writer::new();value.write(&mut body).unwrap();assert_eq!(body.0,wire("beacon"));
    r.open_window(9,"minecraft:enchanting_table".into(),"Enchant".into(),0,&mut sim);r.window_items(9,vec![Some(slot([276,1,0])),Some(slot([351,3,4]))],&mut sim).unwrap();
    r.extra.window.as_mut().unwrap().properties.insert(2,30);
    gui.special=Default::default();gui.enchanting=Some([0,0,30]);gui.container_view=r.current_window_view();sim.xp.level=30;
    assert!(screen(&gui,&sim).buttons.iter().any(|b|b.enabled && matches!(b.action,Action::Enchant(2))));
    r.enchant_choice(2,&sim).unwrap();let Command::Enchant(id,choice)=queue.try_recv().unwrap()else{panic!()};assert_eq!([id,choice].as_slice(),wire("enchant"));
    for signed in [false,true]{
        let held=Stack::new(386,1);let mut book=ui::server_book::Book::from_stack(&held).unwrap();book.author="Player".into();book.insert("책",res,&settings);
        if signed{book.signing=true;book.insert("게임",res,&settings);}
        gui.open(Screen::Book);gui.book=book;
        assert!(screen(&gui,&sim).buttons.iter().any(|b|b.enabled&&matches!(b.action,Action::BookFinish(s) if s==signed)));
        let stack=gui.book.finish(&held,signed).unwrap();
        let mut body=Writer::new();Payload::Book{signed,slot:to_slot(&stack).unwrap()}.write(&mut body).unwrap();
        let expected=wire(if signed{"sign"}else{"edit"});
        let mut a=Reader::new(&body.0);let mut b=Reader::new(&expected);assert_eq!(a.string(20).unwrap(),b.string(20).unwrap());
        // NBTTagCompound's map ordering is arbitrary; all fields and numeric widths must match.
        assert_eq!(from_slot(&a.slot().unwrap().unwrap()).unwrap(),from_slot(&b.slot().unwrap().unwrap()).unwrap());
        assert!(a.remaining().is_empty() && b.remaining().is_empty());
    }
}

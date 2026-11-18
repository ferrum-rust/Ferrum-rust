use super::*;

/// This uses the real integrated server, credentials-free, bound only to localhost.
#[test]
#[ignore = "run python scripts/test_vanilla_loopback.py to start the installed original server"]
fn installed_189_server_round_trip() {
    let port = std::env::var("MC189_LOOPBACK_PORT").expect("run scripts/test_vanilla_loopback.py");
    let port: u16 = port.parse().unwrap();
    let address = format!("127.0.0.1:{port}");
    let status = status(&address, Duration::from_secs(8)).unwrap();
    assert_eq!(status.json["version"]["protocol"], 47);
    let preferences = ClientPreferences::from(&crate::settings::Settings {
        render_distance: 2,
        ..Default::default()
    });
    let client = Client::connect(&address, "Rust189", preferences).unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    let (mut login, mut join, mut correction, mut chunk) = (false, false, false, false);
    let (mut sent, mut chat, mut completion, mut difficulty) = (false, false, false, false);
    while let Some(remaining) = until.checked_duration_since(Instant::now()) {
        match client.recv_timeout(remaining).unwrap() {
            Event::Login { username, .. } => {
                assert_eq!(username, "Rust189");
                login = true;
            }
            Event::JoinGame {
                flat_world,
                dimension,
                ..
            } => {
                assert!(flat_world);
                assert_eq!(dimension, 0);
                client.send(Command::JoinAck).unwrap();
                join = true;
            }
            Event::Position {
                pose,
                correction_id,
                ..
            } => {
                assert!(pose.y.is_finite() && pose.y > 0.);
                client
                    .send(Command::TeleportAck {
                        pose,
                        correction_id,
                    })
                    .unwrap();
                client
                    .send(Command::Movement {
                        pose,
                        correction_id,
                    })
                    .unwrap();
                correction = true;
            }
            Event::Chunk(data) if !data.unload() && data.ground_up => {
                assert_eq!(data.blocks.len(), 65536);
                assert_eq!(data.blocks[0], 7 << 4);
                chunk = true;
            }
            Event::Chat { json, position } if json.contains("ferrum-loopback-189") => {
                assert_eq!(position, 0);
                assert!(json.contains("Rust189"));
                chat = true;
            }
            Event::TabComplete(names) => {
                assert!(names.iter().any(|n| n == "/help"), "{names:?}");
                completion = true;
            }
            Event::WorldAction(play_events::Update::Difficulty(2)) => difficulty = true,
            Event::Disconnect(reason) => panic!("Original server disconnected: {reason}"),
            _ => {}
        }
        if login && join && correction && !sent {
            client
                .send(Command::Chat("ferrum-loopback-189".into()))
                .unwrap();
            client
                .send(Command::TabComplete {
                    input: "/he".into(),
                    target: None,
                })
                .unwrap();
            sent = true;
        }
        if login && join && correction && chunk && chat && completion && difficulty {
            println!("Protocol 47 original server: status, login, flat overworld, S08/C06, chunk, S41 Normal, C01/S02 chat and C14/S3A /help passed");
            return;
        }
    }
    panic!("Timed out: login={login} join={join} teleport={correction} chunk={chunk} chat={chat} tab={completion} difficulty={difficulty}");
}

#[test]
fn original_catalog_all_74_clientbound_play_packets_enter_real_decoders() {
    let rows: Vec<(i32, String, String)> = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-clientbound-catalog.json"
    ))
    .unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_server, _) = listener.accept().unwrap();
    let (events, _receiver) = mpsc::sync_channel(256);
    let mut session = Session::new(
        stream,
        events,
        (&crate::settings::Settings::default()).into(),
    );
    session.play = true;

    assert_eq!(rows.len(), 74);
    for (index, (id, name, original)) in rows.into_iter().enumerate() {
        assert_eq!(id, index as i32, "original registry must be contiguous");
        let data = original
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let result = session.packet(Packet { id, data });
        if id == 0x40 {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("catalog-74-complete"));
        } else {
            result.unwrap_or_else(|err| panic!("original {name} (0x{id:02x}): {err:#}"));
        }
        let error = session
            .packet(Packet { id, data: vec![] })
            .unwrap_err()
            .to_string();
        assert!(
            !error.contains("Unknown protocol-47"),
            "{name} hit the fallback"
        );
    }
    assert!(session
        .packet(Packet {
            id: 0x4a,
            data: vec![]
        })
        .unwrap_err()
        .to_string()
        .contains("Unknown protocol-47"));
}

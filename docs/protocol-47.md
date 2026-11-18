# Protocol 47 연결 범위

Rust 세션은 Minecraft Java 1.8.9의 protocol 47을 사용한다. Play 상태의 서버→클라이언트 ID 74종에 각각 디코더 또는 제어 처리가 있다. 프레임을 읽는 범위와 각 패킷이 표현하는 게임 기능의 완성도는 다르다. 아래 표는 전자를 기록한다.

## 실행

```bat
start.bat --server-status 127.0.0.1:25565
start.bat --connect 127.0.0.1:25565 --username FerrumPlayer
```

상태 조회는 서버의 버전, 설명, 접속자 정보와 ping 왕복 시간을 출력한다. 온라인 인증 서버에 접속하려면 실행하는 프로세스의 환경 변수 `FERRUM189_MC_ACCESS_TOKEN`에 유효한 Minecraft Services 액세스 토큰을, `FERRUM189_MC_PROFILE_ID`에 같은 프로필의 UUID를 설정한다. 게임 내 이름은 `--username` 또는 `FERRUM189_MC_USERNAME`에 지정한다. 토큰은 명령줄이나 배치 파일에 적지 않고 프로세스에 비공개로 전달한다. Ferrum은 서버의 암호화 요청을 받은 뒤 Mojang 세션 서버에 HTTPS로 접속 승인을 요청한다. 승인이 완료되면 RSA로 공유 키와 검증 토큰을 암호화하고 이후 게임 트래픽에 AES-128-CFB8을 적용한다. 토큰 발급과 갱신은 별도로 진행해야 한다. 오프라인 모드 개발 서버는 사용자 이름만으로 접속할 수 있다.

호스트 이름, IPv4, `[IPv6]:port`를 사용할 수 있고 기본 포트는 25565다. DNS SRV 레코드는 지원하지 않는다. 메인 메뉴의 멀티플레이 → 직접 연결 또는 사용자가 추가한 서버로 접속할 수 있다. 기본 예시 두 개는 로컬 미리보기다.

## 전송과 서버 상태 반영

- 5바이트 제한 signed VarInt, VarInt21 프레임 길이, big-endian 숫자, UTF-8/UTF-16 길이 제한, 26/12/26비트 블록 좌표를 처리한다. 프레임 상한은 2,097,151바이트, 압축 해제 상한은 2,097,152바이트다.
- 프레임 분할·합쳐진 TCP 수신, zlib 압축 임계값 변경, 선언된 압축 해제 길이 검증, 슬롯의 bounded NBT를 지원한다. 별도 네트워크 스레드가 nonblocking TCP와 제한된 송수신 큐를 사용한다. 큐가 차서 종료되더라도 별도 종료 채널로 원인을 전달한다.
- Keep Alive의 원래 VarInt를 돌려보낸다. 위치 보정의 상대 좌표/각도 플래그를 적용하고 C06으로 즉시 응답한다. 보정 세대 번호가 다른 오래된 이동 명령은 적용하지 않는다.
- 이동은 20 Hz로 보고하며 위치/시선의 변화에 맞게 C03/C04/C05/C06을 선택한다. 위치 변화 제곱 합 0.0009와 20틱 재전송 조건을 사용한다. 서버가 부여한 비행 권한과 속도를 보존하고 착지 시 비행 해제를 보고한다.
- 일반·부분·bulk 청크, 차원별 skylight 배열, unload, 블록 변경을 읽는다. 서버 월드에 로컬 청크 생성/AI/생존 시뮬레이션을 실행하거나 로컬 저장을 쓰지 않는다. 조명/바이옴 바이트는 디코딩하지만 원격 어댑터의 렌더링에는 아직 모두 반영하지 않는다.
- 플레이어의 체력/허기/포만도/경험치/인벤토리/속도, 서버 몹·드롭의 생성/이동/제거, 엔티티 metadata의 일부를 반영한다. 현재 모델이 있는 몹 9종과 아이템 드롭을 렌더링하며 위치 변경은 3틱으로 보간한다.
- 좌클릭은 서버 공격 또는 START/ABORT/FINISH 채굴 요청으로 보낸다. 크리에이티브·즉시 파괴 블록은 START만 사용한다. 서버의 블록 갱신이 최종 상태다. 엔티티 공격/상호작용에는 거리와 블록 가림 검사를 적용한다.
- 일반 인벤토리 좌/우 클릭은 원래 슬롯/NBT와 커서 스택을 보존해서 예측하고 서버 거절 시 재동기화를 기다린다. 창을 닫으면 로컬 커서도 비우며, 서버가 드롭을 처리한다.

## 게임에 반영하는 내용과 한계

- T와 /로 채팅을 열고 전송한다. C14/S3A 자동완성, S02 일반·시스템 메시지와 액션바, 팀·사이드바·타이틀·플레이어 목록을 구분한다. 입력 기록, 서버 번역 컴포넌트와 링크, 마우스 오버도 지원한다.
- 서버가 연 상자·화로·제작·거래창은 창 번호, 슬롯 NBT, 커서, 속성 및 거래 목록을 서버와 맞춘다. 인챈트는 표시된 선택지를 눌렀을 때 C11, 간판은 편집을 마쳤을 때 C12를 보낸다. 특수 창은 외관이나 슬롯 규칙이 원본과 다를 수 있다.
- 폭발의 블록 파괴와 밀림, 경험치 구슬, 그림, 낙뢰, 메모리 안의 지도 부분 갱신, 관전 카메라, 침대, 노트 블록, 서버 난이도 변경을 처리한다. 통계 및 상인·책 등 바닐라 플러그인 채널을 해석한다. S49는 NBT를 확인한 뒤 원본 기본 엔티티와 같은 빈 콜백 동작을 따른다.
- S48 리소스팩은 서버별 설정에 따라 물어보거나 수락·거절한다. HTTP(S) 파일은 50 MiB 상한과 유효한 SHA-1이 있을 때의 해시 확인을 거쳐 세션에만 적용하며 C19에 결과를 보낸다. 원격 서버가 지정한 로컬 파일 경로는 열지 않는다.
- 보행 중에는 시뮬레이션 틱마다 C03/04/05/06을 선택한다. 탑승 입력과 시선은 C05 다음 C0C로 전송한다. 원본과 다른 부분에는 탈것 물리, 여러 몹의 모델과 행동, 장비·스킨·입자·블록 이벤트의 일부, Forge 및 외부 플러그인 전용 통신이 있다. Microsoft 계정에서 새 토큰을 발급하는 UI도 없다. 공개 서버와 특정 안티치트의 허용 여부는 검증되지 않았다.

## 검증과 원본 근거

```powershell
cargo test --locked protocol::tests
cargo test --locked network::tests
cargo test --locked remote::tests
```

실제 설치된 JAR의 통합 서버를 루프백 주소에 띄우는 검사는 `python scripts/test_vanilla_loopback.py`로 재현한다. 상태·로그인, 청크 수신, 순간이동 응답, 한국어 포함 원본 채팅 패킷 직렬화, 서버 채팅 왕복과 탭 완성은 각각 별도로 검사한다. 온라인 모드 RSA/CFB8 및 암호화 후 압축 프레임 왕복은 테스트용 키와 세션을 쓰는 로컬 피어로 검사하며 실제 계정 로그인을 대신하지 않는다. 단일·부분 청크, 인벤토리, 위치·충돌, 압축 경계, 패킷 필드·잘린 데이터 등의 검사는 Cargo 테스트로 실행한다.

패킷 등록 순서와 필드는 사용자가 설치한 1.8.9 JAR을 `decompile.bat`으로 준비한 로컬 `EnumConnectionState`, `PacketBuffer`, `NettyCompressionDecoder`, 각 `Cxx/Sxx` 패킷, `NetHandlerPlayClient`, `PlayerControllerMP`, `Container`와 대조했다. 디컴파일한 Java 소스는 `.local/reference/java`에만 두고 공개 배포에 포함하지 않는다.

## 전체 ID 목록

설치된 1.8.9 JAR의 Play 레지스트리에서 얻은 [수신 패킷 74종의 실제 바이트](../tests/fixtures/vanilla189-clientbound-catalog.json)를 Rust 세션에 입력한다. 각 ID에 유효한 사례가 하나씩 있으며, S40은 정상적으로 연결 종료 사유를 전달한다. `python scripts/test_vanilla_clientbound_catalog.py`로 원본 표본을 다시 만들 수 있다. 좌표 보정, 채팅, 창 클릭, 점수판 등 변형이 많은 부분은 이 검사와 별도로 여러 입력을 비교한다.

ID는 상태와 방향의 조합 안에서만 유일하다. `decoded/control`은 wire 필드를 읽거나 연결 응답을 처리한다는 뜻으로, 화면과 모든 게임 동작을 완성했다는 뜻이 아니다. `encoder`는 해당 동작의 송신 경로다.

### Handshake

| 방향 | ID | 원본 클래스 | 현재 범위 |
|---|---|---|---|
| Serverbound | `0x00` | `C00Handshake` | encoder |

### Status

| 방향 | ID | 원본 클래스 | 현재 범위 |
|---|---|---|---|
| Serverbound | `0x00` | `C00PacketServerQuery` | encoder |
| Clientbound | `0x00` | `S00PacketServerInfo` | decoded/control |
| Serverbound | `0x01` | `C01PacketPing` | encoder |
| Clientbound | `0x01` | `S01PacketPong` | decoded/control |

### Login

| 방향 | ID | 원본 클래스 | 현재 범위 |
|---|---|---|---|
| Clientbound | `0x00` | `S00PacketDisconnect` | decoded/control |
| Clientbound | `0x01` | `S01PacketEncryptionRequest` | decoded/control |
| Clientbound | `0x02` | `S02PacketLoginSuccess` | decoded/control |
| Clientbound | `0x03` | `S03PacketEnableCompression` | decoded/control |
| Serverbound | `0x00` | `C00PacketLoginStart` | encoder |
| Serverbound | `0x01` | `C01PacketEncryptionResponse` | encoder |

### Play

| 방향 | ID | 원본 클래스 | 현재 범위 |
|---|---|---|---|
| Clientbound | `0x00` | `S00PacketKeepAlive` | decoded/control |
| Clientbound | `0x01` | `S01PacketJoinGame` | decoded/control |
| Clientbound | `0x02` | `S02PacketChat` | decoded/control |
| Clientbound | `0x03` | `S03PacketTimeUpdate` | decoded/control |
| Clientbound | `0x04` | `S04PacketEntityEquipment` | decoded/control |
| Clientbound | `0x05` | `S05PacketSpawnPosition` | decoded/control |
| Clientbound | `0x06` | `S06PacketUpdateHealth` | decoded/control |
| Clientbound | `0x07` | `S07PacketRespawn` | decoded/control |
| Clientbound | `0x08` | `S08PacketPlayerPosLook` | decoded/control |
| Clientbound | `0x09` | `S09PacketHeldItemChange` | decoded/control |
| Clientbound | `0x0a` | `S0APacketUseBed` | decoded/control |
| Clientbound | `0x0b` | `S0BPacketAnimation` | decoded/control |
| Clientbound | `0x0c` | `S0CPacketSpawnPlayer` | decoded/control |
| Clientbound | `0x0d` | `S0DPacketCollectItem` | decoded/control |
| Clientbound | `0x0e` | `S0EPacketSpawnObject` | decoded/control |
| Clientbound | `0x0f` | `S0FPacketSpawnMob` | decoded/control |
| Clientbound | `0x10` | `S10PacketSpawnPainting` | decoded/control |
| Clientbound | `0x11` | `S11PacketSpawnExperienceOrb` | decoded/control |
| Clientbound | `0x12` | `S12PacketEntityVelocity` | decoded/control |
| Clientbound | `0x13` | `S13PacketDestroyEntities` | decoded/control |
| Clientbound | `0x14` | `S14PacketEntity` | decoded/control |
| Clientbound | `0x15` | `S14PacketEntity.S15PacketEntityRelMove` | decoded/control |
| Clientbound | `0x16` | `S14PacketEntity.S16PacketEntityLook` | decoded/control |
| Clientbound | `0x17` | `S14PacketEntity.S17PacketEntityLookMove` | decoded/control |
| Clientbound | `0x18` | `S18PacketEntityTeleport` | decoded/control |
| Clientbound | `0x19` | `S19PacketEntityHeadLook` | decoded/control |
| Clientbound | `0x1a` | `S19PacketEntityStatus` | decoded/control |
| Clientbound | `0x1b` | `S1BPacketEntityAttach` | decoded/control |
| Clientbound | `0x1c` | `S1CPacketEntityMetadata` | decoded/control |
| Clientbound | `0x1d` | `S1DPacketEntityEffect` | decoded/control |
| Clientbound | `0x1e` | `S1EPacketRemoveEntityEffect` | decoded/control |
| Clientbound | `0x1f` | `S1FPacketSetExperience` | decoded/control |
| Clientbound | `0x20` | `S20PacketEntityProperties` | decoded/control |
| Clientbound | `0x21` | `S21PacketChunkData` | decoded/control |
| Clientbound | `0x22` | `S22PacketMultiBlockChange` | decoded/control |
| Clientbound | `0x23` | `S23PacketBlockChange` | decoded/control |
| Clientbound | `0x24` | `S24PacketBlockAction` | decoded/control |
| Clientbound | `0x25` | `S25PacketBlockBreakAnim` | decoded/control |
| Clientbound | `0x26` | `S26PacketMapChunkBulk` | decoded/control |
| Clientbound | `0x27` | `S27PacketExplosion` | decoded/control |
| Clientbound | `0x28` | `S28PacketEffect` | decoded/control |
| Clientbound | `0x29` | `S29PacketSoundEffect` | decoded/control |
| Clientbound | `0x2a` | `S2APacketParticles` | decoded/control |
| Clientbound | `0x2b` | `S2BPacketChangeGameState` | decoded/control |
| Clientbound | `0x2c` | `S2CPacketSpawnGlobalEntity` | decoded/control |
| Clientbound | `0x2d` | `S2DPacketOpenWindow` | decoded/control |
| Clientbound | `0x2e` | `S2EPacketCloseWindow` | decoded/control |
| Clientbound | `0x2f` | `S2FPacketSetSlot` | decoded/control |
| Clientbound | `0x30` | `S30PacketWindowItems` | decoded/control |
| Clientbound | `0x31` | `S31PacketWindowProperty` | decoded/control |
| Clientbound | `0x32` | `S32PacketConfirmTransaction` | decoded/control |
| Clientbound | `0x33` | `S33PacketUpdateSign` | decoded/control |
| Clientbound | `0x34` | `S34PacketMaps` | decoded/control |
| Clientbound | `0x35` | `S35PacketUpdateTileEntity` | decoded/control |
| Clientbound | `0x36` | `S36PacketSignEditorOpen` | decoded/control |
| Clientbound | `0x37` | `S37PacketStatistics` | decoded/control |
| Clientbound | `0x38` | `S38PacketPlayerListItem` | decoded/control |
| Clientbound | `0x39` | `S39PacketPlayerAbilities` | decoded/control |
| Clientbound | `0x3a` | `S3APacketTabComplete` | decoded/control |
| Clientbound | `0x3b` | `S3BPacketScoreboardObjective` | decoded/control |
| Clientbound | `0x3c` | `S3CPacketUpdateScore` | decoded/control |
| Clientbound | `0x3d` | `S3DPacketDisplayScoreboard` | decoded/control |
| Clientbound | `0x3e` | `S3EPacketTeams` | decoded/control |
| Clientbound | `0x3f` | `S3FPacketCustomPayload` | decoded/control |
| Clientbound | `0x40` | `S40PacketDisconnect` | decoded/control |
| Clientbound | `0x41` | `S41PacketServerDifficulty` | decoded/control |
| Clientbound | `0x42` | `S42PacketCombatEvent` | decoded/control |
| Clientbound | `0x43` | `S43PacketCamera` | decoded/control |
| Clientbound | `0x44` | `S44PacketWorldBorder` | decoded/control |
| Clientbound | `0x45` | `S45PacketTitle` | decoded/control |
| Clientbound | `0x46` | `S46PacketSetCompressionLevel` | decoded/control |
| Clientbound | `0x47` | `S47PacketPlayerListHeaderFooter` | decoded/control |
| Clientbound | `0x48` | `S48PacketResourcePackSend` | decoded/control |
| Clientbound | `0x49` | `S49PacketUpdateEntityNBT` | decoded/control |
| Serverbound | `0x00` | `C00PacketKeepAlive` | encoder |
| Serverbound | `0x01` | `C01PacketChatMessage` | encoder |
| Serverbound | `0x02` | `C02PacketUseEntity` | encoder |
| Serverbound | `0x03` | `C03PacketPlayer` | encoder |
| Serverbound | `0x04` | `C03PacketPlayer.C04PacketPlayerPosition` | encoder |
| Serverbound | `0x05` | `C03PacketPlayer.C05PacketPlayerLook` | encoder |
| Serverbound | `0x06` | `C03PacketPlayer.C06PacketPlayerPosLook` | encoder |
| Serverbound | `0x07` | `C07PacketPlayerDigging` | encoder |
| Serverbound | `0x08` | `C08PacketPlayerBlockPlacement` | encoder |
| Serverbound | `0x09` | `C09PacketHeldItemChange` | encoder |
| Serverbound | `0x0a` | `C0APacketAnimation` | encoder |
| Serverbound | `0x0b` | `C0BPacketEntityAction` | encoder |
| Serverbound | `0x0c` | `C0CPacketInput` | encoder |
| Serverbound | `0x0d` | `C0DPacketCloseWindow` | encoder |
| Serverbound | `0x0e` | `C0EPacketClickWindow` | encoder |
| Serverbound | `0x0f` | `C0FPacketConfirmTransaction` | encoder |
| Serverbound | `0x10` | `C10PacketCreativeInventoryAction` | encoder |
| Serverbound | `0x11` | `C11PacketEnchantItem` | encoder |
| Serverbound | `0x12` | `C12PacketUpdateSign` | encoder |
| Serverbound | `0x13` | `C13PacketPlayerAbilities` | encoder |
| Serverbound | `0x14` | `C14PacketTabComplete` | encoder |
| Serverbound | `0x15` | `C15PacketClientSettings` | encoder |
| Serverbound | `0x16` | `C16PacketClientStatus` | encoder |
| Serverbound | `0x17` | `C17PacketCustomPayload` | encoder |
| Serverbound | `0x18` | `C18PacketSpectate` | encoder |
| Serverbound | `0x19` | `C19PacketResourcePackStatus` | encoder |


# 동일한 월드에서 FPS 측정하기

## 청크 스트리밍과 원거리 LOD 측정

현재 그래픽 옵션을 유지한 새 월드 자동 비행은 다음 명령으로 재현한다. 저장 파일을 만들거나 기존 월드를 수정하지 않는다. `--render-distance`는 이 실행에만 적용되며 `options.json`에 기록하지 않는다.

```bat
start.bat --benchmark --benchmark-streaming --creative --seed 8924706420471469774 --render-distance 16 --seconds 20 --benchmark-output-dir reports\flight-16
start.bat --benchmark --benchmark-streaming --benchmark-distant-ready --creative --seed 8924706420471469774 --render-distance 128 --seconds 20 --benchmark-output-dir reports\flight-128
```

128청크 실행은 가까운 24청크의 전체 블록 메시와 그 바깥의 표면 LOD를 사용한다. `--benchmark-distant-ready`는 요청된 원거리 타일을 전부 올린 뒤 예열과 이동 측정을 시작한다. 최초 캐시 생성 시간은 FPS 측정에 포함되지 않으므로 캐시가 없는 첫 방문과 재방문을 구분해서 기록한다. 일반 플레이에서는 이 대기 조건 없이 먼 지형을 점진적으로 표시한다.

각 출력 폴더에는 실제 프레임 간격 `benchmark.json`과 측정 종료 화면 `benchmark-streaming.png`가 생긴다. 화면 캡처 시간은 FPS 샘플에서 제외한다. 새 월드 데이터 로딩과 최초 CPU 메시 시간은 `reports/world.json`에 별도로 기록되며, 다음 실행 전에 복사해야 보존된다. 이 값에는 리소스 준비와 최초 GPU 업로드가 포함되지 않는다.

`frame_seconds`, FPS, `max_work_ms`, `max_render_cpu_ms`는 예열 후 측정 구간의 값이다. 마지막 항목은 화면 획득·명령 기록·제출·표시를 나눈 CPU 벽시계 시간이며, GPU 타임스탬프가 아니라 드라이버 대기도 포함한다. `chunk_phase_calls_total_ms_max_ms`와 `simulation_phases`는 월드 진입부터 누적한 진단값으로, LOD 준비 대기도 포함한다. 긴 이동의 메모리 안정성이나 첫 방문 생성 속도를 짧은 정상 상태 FPS만으로 판단하지 않는다.

## 2026-11-06 측정

AMD Radeon(TM) Graphics에서 원본 1.8.9 Java 8/OpenGL과 Rust/Vulkan 릴리스를 번갈아 두 번씩 실행했다. 같은 Anvil 월드를 읽고 해안에 선 고정 카메라로 1600×900, 8청크, 화려하게, AO 최대로, 밉맵 4, VSync 끔을 적용했다. 각 창에서 8초 이상 예열하고 15초간 측정했다. 몹이 없는 고정 장면의 결과이며 일반 실행에서 선택한 RTX 3050은 이 비교에서 제외했다.

| 실제 창 측정 | 원본 Java | Rust/Vulkan |
|---|---:|---:|
| 첫 실행 평균 FPS | 660.4 | 736.3 |
| 두 번째 실행 평균 FPS | 567.9 | 593.0 |
| 평균 FPS의 두 실행 중앙값 | 614.2 | 664.6 |
| 1% low의 두 실행 중앙값 | 203.3 | 240.0 |

이 장면에서 평균 FPS는 8.2%, 1% low는 18.1% 높았다. 반복 간 편차가 있으므로 이동·청크 생성·몹 전투의 성능은 따로 측정해야 한다. 첫 번째 캡처 쌍의 전체 화면에서 RGB 채널당 평균 절대 차이는 255단계 중 1.40이었다. [측정 원자료와 요약](../reports/benchmark-189-matched/comparison.json) · [원본 화면](../reports/benchmark-189-matched/original-1.png) · [Rust 화면](../reports/benchmark-189-matched/native-1.png)

## 재현 방법

프로젝트 루트의 PowerShell에서 실행한다. 원본 비교에는 설치된 Minecraft 1.8.9 JAR과 리소스, Java 8, JDK 8의 javac가 필요하다. 최초 1회 `decompile.bat`로 검증용 JAR을 준비한다. 테스트 월드 생성에는 NumPy가 필요하다.

```powershell
cargo build --release --locked
$run = (python scripts/benchmark_frames.py prepare | ConvertFrom-Json).run
python scripts/benchmark_frames.py vanilla $run
```

생성기는 시스템 임시 폴더에 625청크의 Anvil 월드를 만든다. 저장 위치는 `prepare --parent E:\bench` 또는 `FERRUM_BENCH_TMP`로 지정할 수 있다. 원본 실행기는 임시 복제본과 로컬 테스트 플레이어를 사용한다. 설치 폴더의 쓰기·삭제와 외부 네트워크 연결을 차단한다.

원본 창이 닫힌 후 출력된 OpenGL JSON 경로를 `$java`에 넣는다. 기록된 GPU 이름을 Vulkan 실행에도 전달한다.

```powershell
$java = Read-Host 'opengl-frames.json 경로'
$gpu = (Get-Content -Raw -LiteralPath $java | ConvertFrom-Json).gpu
python scripts/benchmark_frames.py native $run --adapter $gpu
$rust = Read-Host 'vulkan-frames.json 경로'
python scripts/benchmark_frames.py compare $run --original $java --native $rust --output-dir reports/my-frame-comparison
```

결과 폴더에 `comparison.json`, 각 실행의 원본 프레임 간격 JSON과 화면 PNG, 장면 설정 및 월드 파일 SHA-256 목록이 저장된다. 출력에는 새 디렉터리를 지정한다. 각 실행만 따로 보관하려면 `native` 또는 `vanilla`에 `--output-dir`을 전달한다.

두 창을 차례로 실행하고 각 측정 중에는 빌드나 다른 GPU 작업을 멈춘다. 반복하려면 같은 `$run`에서 다시 측정해 한 쌍마다 `--original`과 `--native`를 하나씩 추가한다. 비교 도구는 개별 프레임 간격으로 평균 FPS와 가장 느린 1% 프레임의 FPS를 다시 계산한다. 반복 요약은 각 클라이언트별 실행 결과의 중앙값이다. 한 장면에서 측정한 평균 FPS와 1% low는 각각 해석한다.

장면은 `scripts/benchmark_scene.json`에 정의되어 있다. 기본 조건은 1600×900, 시야 거리 8청크, 한낮의 고정 카메라, 최소 예열 8초, 측정 15초다. 실시간 게임 창의 프레임 표시 간격을 측정하므로, 두 클라이언트의 실제 렌더링 및 게임 루프 차이도 결과에 포함된다.

"""Run the production Rust protocol session against a disposable original integrated server."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import tempfile
import time

from vanilla_world import ROOT, classpath, java_executable


def test_binary():
    build = subprocess.run(["cargo", "test", "--locked", "--no-run", "--message-format=json"],
                           cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True, encoding="utf8")
    for line in build.stdout.splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item["target"]["name"] == "ferrum189" and item.get("executable"):
            return Path(item["executable"])
    raise RuntimeError("Cargo did not return the Rust test executable")


def render(binary, target, work, jar, port, env):
    client = work / "rust-client"
    client.mkdir()
    local = client / ".local"
    local.mkdir()
    resources = local / "resources"
    source = ROOT / ".local/resources"
    if not (source / "registry.json").is_file():
        raise RuntimeError("The project is missing its imported 1.8.9 resources")
    if os.name == "nt":
        subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command",
                        "New-Item -ItemType Junction -Path $env:FERRUM_LINK -Target $env:FERRUM_RESOURCES | Out-Null"],
                       env={**env, "FERRUM_LINK": str(resources), "FERRUM_RESOURCES": str(source)},
                       check=True, creationflags=subprocess.CREATE_NO_WINDOW)
    else:
        resources.symlink_to(source, target_is_directory=True)
    (client / "options.json").write_text(json.dumps({
        "language": "ko_kr", "gui_scale": 2, "volume": 0.0, "render_distance": 6,
        "menu": {"snooper": False},
    }), encoding="utf8")
    profile = work / "rust-profile"
    profile.mkdir()
    capture = work / "server-frame.png"
    logs = work / "render-client.log"
    command = [binary, "--connect", f"127.0.0.1:{port}", "--username", "RustVulkan189",
               "--fresh", "--language", "ko_kr", "--render-distance", "6",
               "--jar", jar, "--screenshot", capture]
    target.parent.mkdir(parents=True, exist_ok=True)
    try:
        with logs.open("w", encoding="utf8") as output:
            subprocess.run(command, cwd=client, env={**env, "APPDATA": str(profile)},
                           stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                           timeout=70, check=True,
                           creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        data = capture.read_bytes()
        if data[:8] != b"\x89PNG\r\n\x1a\n" or struct.unpack(">II", data[16:24]) != (1600, 900):
            raise RuntimeError("Vulkan capture did not produce a 1600x900 PNG")
        host = (work / "server.log").read_text(encoding="utf8", errors="replace")
        if not any("RustVulkan189" in line and ("logged in" in line or "joined the game" in line)
                   for line in host.splitlines()):
            raise RuntimeError("Missing RustVulkan189 join in the original server log")
        shutil.copyfile(capture, target)
        print(f"Joined the original server and captured {target}", flush=True)
    finally:
        if logs.is_file():
            shutil.copyfile(logs, target.with_suffix(".client.log"))
        shutil.copyfile(work / "server.log", target.with_suffix(".server.log"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=Path)
    probe = parser.add_mutually_exclusive_group()
    probe.add_argument("--plugin-probe",action="store_true",help="Reject a predicted click in a named Java chest menu and accept the following click")
    probe.add_argument("--vehicle-probe",action="store_true",help="Ride and jump a saddled horse on the original server")
    probe.add_argument("--window-probe", action="store_true", help="Open the original anvil and rename a sword through the Rust client")
    probe.add_argument("--mob-probe",action="store_true",help="Verify aiming at and attacking an original iron golem")
    parser.add_argument("--render-client", type=Path, help="Capture the normal Vulkan client joining this server")
    parser.add_argument("--screenshot", type=Path, help="Output PNG (requires --render-client)")
    args = parser.parse_args()
    if args.screenshot and not args.render_client:
        parser.error("--screenshot requires --render-client")
    # A render-only invocation needs no test executable or compiler invocation.
    binary = args.test_binary or (None if args.render_client else test_binary())
    if binary:
        binary = binary.resolve()
    test = ("remote::plugin_probe::installed_189_plugin_chest_reject_resync_and_accept" if args.plugin_probe else
            "remote::tests::installed_189_horse_ride" if args.vehicle_probe else
            "remote::tests::installed_189_anvil_rename" if args.window_probe else
            "remote::tests::installed_189_mob_attack" if args.mob_probe else
            "network::loopback_tests::installed_189_server_round_trip")
    if binary:
        listed = subprocess.check_output([binary, "--list", test], cwd=ROOT, text=True)
        if test + ": test" not in listed:
            raise RuntimeError(f"{binary} does not contain {test}")
    version = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9"
    cp = classpath(version / "1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    work = Path(tempfile.mkdtemp(prefix="original-loopback-"))
    game, classes = work / "game", work / "classes"
    game.mkdir()
    classes.mkdir()
    print("Loopback test game and log:", work, flush=True)
    sources = [str(ROOT / "tools/java" / (name+".java")) for name in
               ("VanillaFrameBenchmark", "MenuOracle", "LoopbackServerOracle")]
    subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", classes, *sources], cwd=work, check=True)
    with socket.socket() as reserve:
        reserve.bind(("127.0.0.1", 0))
        port = str(reserve.getsockname()[1])
    natives = version / "natives-windows-x86_64"
    ready = work / "ready"
    command = [java_executable(), "-Xmx512M", f"-Djava.library.path={natives}",
               f"-Dorg.lwjgl.librarypath={natives}", f"-Duser.home={work}", f"-Djava.io.tmpdir={work}",
               "-Dferrum.pluginProbe="+str(args.plugin_probe).lower(), "-Dferrum.vehicleProbe="+str(args.vehicle_probe).lower(), "-Dferrum.windowProbe="+str(args.window_probe).lower(),
               "-Dferrum.mobProbe="+str(args.mob_probe).lower(), "-cp", str(classes)+os.pathsep+cp, "LoopbackServerOracle", game, version.parents[1]/"assets", ready, port]
    with (work/"server.log").open("w", encoding="utf8") as log, subprocess.Popen(
            command, cwd=work, stdout=log, stderr=subprocess.STDOUT,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)) as server:
        try:
            deadline = time.monotonic()+100
            while not ready.exists():
                if server.poll() is not None or time.monotonic()>=deadline:
                    raise RuntimeError(f"Original server failed to start. See {work/'server.log'}")
                time.sleep(0.1)
            env = {key: value for key, value in os.environ.items() if not key.startswith("FERRUM189_MC_")}
            if binary:
                subprocess.run([binary, test, "--exact", "--ignored", "--nocapture"], cwd=ROOT,
                               env={**env, "MC189_LOOPBACK_PORT": port}, timeout=45, check=True)
            if args.render_client:
                target = (args.screenshot or ROOT / "reports/server-loopback-189.png").resolve()
                render(args.render_client.resolve(), target, work, version / "1.8.9.jar", port, env)
        finally:
            (work/"stop").touch()
            try:
                server.wait(10)
            except subprocess.TimeoutExpired:
                server.kill()


if __name__ == "__main__":
    main()

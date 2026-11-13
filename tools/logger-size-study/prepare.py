"""Prepare disposable sources for offline size probes; never touches hardware."""
import argparse
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[2]


def prepare(destination, passive, relocate):
    destination = Path(destination).resolve()
    if ROOT / "target" not in destination.parents:
        raise ValueError("Use a new directory beneath target/ for disposable sources")
    destination.mkdir(parents=True, exist_ok=False)
    for name in ("firmware", "firmware-core", "firmware-image", "firmware-pack",
                 "flasher", "fusb302-pd", ".cargo"):
        shutil.copytree(ROOT / name, destination / name)
    for name in ("Cargo.toml", "Cargo.lock"):
        shutil.copyfile(ROOT / name, destination / name)
    if passive:
        path = destination / "firmware/src/pd_task.rs"
        source = path.read_text(encoding="utf-8")
        assert source.count("let mut sink_mode = true;") == 1
        source = source.replace("let mut sink_mode = true;", "let mut sink_mode = false;")
        start = source.index('    if sink_mode {\n        // `pd-boot-diagnostic`')
        end = source.index("    loop {\n        observation.irq_high", start)
        source = source[:start] + """    // SIZE PROBE ONLY: no active PD sink awaits in the generated task.
    if let Some(released) = passthrough(&mut observation).await {
        sniff(&mut observation, released).await;
    }
""" + source[end:]
        path.write_text(source, encoding="utf-8")
    if relocate:
        path = destination / "firmware-pack/src/main.rs"
        source = path.read_text(encoding="utf-8")
        old = """let stream_start = if stage == Stage::Main {
        APP_LIMIT
            .checked_sub(main_packed.len())"""
        new = """let stream_start = if matches!(stage, Stage::Main | Stage::Complete) {
        APP_LIMIT
            .checked_sub(main_packed.len() + upper_packed.len())"""
        assert old in source
        path.write_text(source.replace(old, new), encoding="utf-8")
    (destination / "SIZE-PROBE-ONLY.txt").write_text(
        "NOT FOR FLASHING. UI and other trigger paths are not adapted or qualified.\n")
    print(destination)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--passive", action="store_true")
    parser.add_argument("--relocate-packer", action="store_true")
    args = parser.parse_args()
    prepare(args.destination, args.passive, args.relocate_packer)

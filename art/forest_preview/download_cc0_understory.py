"""Fetch the exact Poly Haven 1K glTF dependency sets for forest foreground QA.

Sources stay in the ignored target directory. The tracked manifest records the
public source, CC0 license, file sizes and independently checked SHA-256 hashes.
This script deliberately has a different output from download_cc0.py (trees).
"""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor, as_completed
from hashlib import md5, sha256
from json import dumps, loads
from pathlib import Path
from urllib.parse import urlparse
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[2]
META = ROOT / "target/world-preview"
OUTPUT = META / "cc0-sources"
REPORT = ROOT / "art/forest_preview/cc0_understory_downloads.json"
ASSETS = (
    "shrub_03",
    "tree_stump_01",
    "grass_medium_01",
    "rock_moss_set_01",
    "dandelion_01",
    "flower_heliophila",
)
USER_AGENT = "Warbell-Forest-Art-Study/1.0 (https://github.com/miskibin/warbell)"


def get_bytes(url: str) -> bytes:
    request = Request(url, headers={"User-Agent": USER_AGENT})
    with urlopen(request, timeout=120) as response:
        return response.read()


def metadata(asset: str, kind: str) -> tuple[dict, str]:
    url = f"https://api.polyhaven.com/{kind}/{asset}"
    path = META / f"{asset}-{kind}.json"
    if not path.exists():
        path.write_bytes(get_bytes(url))
    data = path.read_bytes()
    return loads(data), sha256(data).hexdigest()


def safe_relative(name: str) -> Path:
    relative = Path(name)
    if relative.is_absolute() or ".." in relative.parts or not relative.parts:
        raise ValueError(f"unsafe dependency path: {name}")
    return relative


def download(asset: str, name: str, spec: dict) -> dict:
    url = spec["url"]
    if urlparse(url).hostname != "dl.polyhaven.org":
        raise ValueError(f"unexpected download host: {url}")
    dest = OUTPUT / asset / safe_relative(name)
    dest.parent.mkdir(parents=True, exist_ok=True)
    temp = dest.with_name(dest.name + ".part")
    if not dest.exists() or md5(dest.read_bytes()).hexdigest() != spec["md5"]:
        request = Request(url, headers={"User-Agent": USER_AGENT})
        with urlopen(request, timeout=120) as response, temp.open("wb") as handle:
            while chunk := response.read(1024 * 1024):
                handle.write(chunk)
        if temp.stat().st_size != spec["size"]:
            raise ValueError(f"size mismatch: {asset}/{name}")
        if md5(temp.read_bytes()).hexdigest() != spec["md5"]:
            raise ValueError(f"MD5 mismatch: {asset}/{name}")
        temp.replace(dest)
    raw = dest.read_bytes()
    if len(raw) != spec["size"] or md5(raw).hexdigest() != spec["md5"]:
        raise ValueError(f"cached dependency failed validation: {asset}/{name}")
    return {
        "path": dest.relative_to(ROOT).as_posix(),
        "url": url,
        "bytes": len(raw),
        "source_md5": spec["md5"],
        "sha256": sha256(raw).hexdigest(),
    }


def main() -> None:
    META.mkdir(parents=True, exist_ok=True)
    jobs = []
    assets = []
    for asset in ASSETS:
        files, files_sha = metadata(asset, "files")
        info, info_sha = metadata(asset, "info")
        gltf = files["gltf"]["1k"]["gltf"]
        dependencies = {f"{asset}_1k.gltf": gltf, **gltf["include"]}
        # Poly Haven's glTF references an opaque JPG diffuse even for MASK/BLEND
        # foliage. Preserve its separate 1K alpha PNG for Blender reattachment;
        # without it, a glTF import renders solid rectangular leaf cards.
        alpha = files.get("Alpha", {}).get("1k", {}).get("png")
        if alpha:
            dependencies[f"textures/{asset}_alpha_1k.png"] = alpha
        assets.append(
            {
                "id": asset,
                "page": f"https://polyhaven.com/a/{asset}",
                "license": "CC0-1.0",
                "license_page": "https://polyhaven.com/license",
                "files_api": f"https://api.polyhaven.com/files/{asset}",
                "files_api_sha256": files_sha,
                "info_api": f"https://api.polyhaven.com/info/{asset}",
                "info_api_sha256": info_sha,
                "info_name": info.get("name"),
                "files": [],
            }
        )
        jobs.extend((asset, name, spec) for name, spec in dependencies.items())

    with ThreadPoolExecutor(max_workers=4) as pool:
        pending = {pool.submit(download, *job): job[0] for job in jobs}
        by_asset = {asset["id"]: asset for asset in assets}
        for future in as_completed(pending):
            by_asset[pending[future]]["files"].append(future.result())

    for asset in assets:
        asset["files"].sort(key=lambda file: file["path"])
    report = {
        "schema": "warbell.forest_cc0_understory_sources.v1",
        "request_user_agent": USER_AGENT,
        "assets": assets,
    }
    REPORT.write_text(dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        dumps(
            {
                "assets": len(assets),
                "files": len(jobs),
                "bytes": sum(file["bytes"] for asset in assets for file in asset["files"]),
                "manifest": REPORT.relative_to(ROOT).as_posix(),
            }
        )
    )


if __name__ == "__main__":
    main()

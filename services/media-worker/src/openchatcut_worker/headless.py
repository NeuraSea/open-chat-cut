from __future__ import annotations

import hashlib
import ipaddress
import base64
import math
import os
import json
import re
import subprocess
import struct
import uuid
import zipfile
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Callable
from urllib.parse import quote, urlencode, urlparse

from .errors import CapabilityUnavailable, WorkerError
from .hardware import h264_encoder_arguments
from .security import resolve_under_root, safe_output_path, sanitized_environment


Progress = Callable[[float, str], None]


def _loopback_http_origin(value: Any) -> str:
    if not isinstance(value, str):
        raise WorkerError("INVALID_EDITOR_URL", "The trusted editor URL is required")
    parsed = urlparse(value)
    if (
        parsed.scheme != "http"
        or parsed.username is not None
        or parsed.password is not None
        or parsed.path not in ("", "/")
        or parsed.params
        or parsed.query
        or parsed.fragment
        or parsed.port is None
    ):
        raise WorkerError("INVALID_EDITOR_URL", "The editor URL must be an HTTP loopback origin")
    host = parsed.hostname
    if host is None:
        raise WorkerError("INVALID_EDITOR_URL", "The editor URL has no host")
    try:
        loopback = ipaddress.ip_address(host).is_loopback
    except ValueError:
        loopback = host.lower() == "localhost"
    if not loopback:
        raise WorkerError("INVALID_EDITOR_URL", "The editor URL must use a loopback host")
    display_host = f"[{host}]" if ":" in host else host
    return f"http://{display_host}:{parsed.port}"


def _is_allowed_browser_url(value: str) -> bool:
    parsed = urlparse(value)
    if parsed.scheme in ("about", "blob", "data"):
        return True
    if parsed.scheme not in ("http", "ws") or parsed.hostname is None:
        return False
    try:
        return ipaddress.ip_address(parsed.hostname).is_loopback
    except ValueError:
        return parsed.hostname.lower() == "localhost"


def _find_system_chromium() -> Path | None:
    candidates = [
        Path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        Path("/Applications/Chromium.app/Contents/MacOS/Chromium"),
        Path("/usr/bin/google-chrome"),
        Path("/usr/bin/google-chrome-stable"),
        Path("/usr/bin/chromium"),
        Path("/usr/bin/chromium-browser"),
        Path.home() / "AppData/Local/Google/Chrome/Application/chrome.exe",
        Path(os.environ.get("PROGRAMFILES", "C:/Program Files"))
        / "Google/Chrome/Application/chrome.exe",
        Path(os.environ.get("PROGRAMFILES(X86)", "C:/Program Files (x86)"))
        / "Google/Chrome/Application/chrome.exe",
    ]
    return next((candidate for candidate in candidates if candidate.is_file()), None)


def _png_dimensions(path: Path) -> tuple[int, int]:
    with path.open("rb") as source:
        header = source.read(24)
    if len(header) < 24 or not header.startswith(b"\x89PNG\r\n\x1a\n"):
        raise WorkerError("INVALID_PREVIEW_ARTIFACT", "Chromium did not produce a PNG image")
    width, height = struct.unpack(">II", header[16:24])
    if width <= 0 or height <= 0 or width > 16_384 or height > 16_384:
        raise WorkerError("INVALID_PREVIEW_ARTIFACT", "Preview PNG dimensions are unsafe")
    return width, height


def capture_web_page(
    *,
    source: Path,
    destination: Path,
    source_url: Any,
    asset_paths: list[Path],
) -> dict[str, Any]:
    """Render untrusted public HTML in an offline, script-disabled origin."""

    try:
        from playwright.sync_api import sync_playwright
    except ImportError as error:
        raise CapabilityUnavailable(
            "web-capture",
            "Install openchatcut-media-worker[render] and run `python -m playwright install chromium`",
        ) from error
    if not isinstance(source_url, str):
        raise WorkerError("INVALID_WEB_CAPTURE_REQUEST", "The validated source URL is required")
    parsed_source = urlparse(source_url)
    if (
        parsed_source.scheme not in ("http", "https")
        or parsed_source.hostname is None
        or parsed_source.username is not None
        or parsed_source.password is not None
        or parsed_source.fragment
    ):
        raise WorkerError("INVALID_WEB_CAPTURE_REQUEST", "The source URL is not safe")
    if source.stat().st_size > 4 * 1024 * 1024:
        raise WorkerError("WEB_CAPTURE_HTML_TOO_LARGE", "Website HTML exceeds four MiB")
    try:
        html = source.read_text(encoding="utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise WorkerError("INVALID_WEB_CAPTURE_HTML", "Website HTML is not UTF-8") from error
    if "\x00" in html:
        raise WorkerError("INVALID_WEB_CAPTURE_HTML", "Website HTML contains NUL bytes")

    # The daemon downloaded this document and every staged public asset through
    # its DNS-pinned SSRF policy. Chromium receives bytes only: no page script,
    # service worker, navigation, remote font, frame, or subresource is allowed.
    csp = (
        "<meta http-equiv=\"Content-Security-Policy\" content=\""
        "default-src 'none'; style-src 'unsafe-inline'; img-src data: blob:; "
        "font-src data:; media-src 'none'; frame-src 'none'; object-src 'none'; "
        "base-uri 'none'; form-action 'none'\">"
    )
    guarded_html = csp + html
    blocked_requests = 0
    browser = None
    try:
        with sync_playwright() as playwright:
            executable = _find_system_chromium()
            launch_options: dict[str, Any] = {
                "headless": True,
                "args": [
                    "--disable-background-networking",
                    "--disable-component-update",
                    "--disable-default-apps",
                    "--disable-sync",
                    "--metrics-recording-only",
                    "--no-first-run",
                    "--no-default-browser-check",
                    "--renderer-process-limit=1",
                ],
            }
            if executable is not None:
                launch_options["executable_path"] = str(executable)
            try:
                browser = playwright.chromium.launch(**launch_options)
            except Exception as error:
                raise CapabilityUnavailable(
                    "web-capture",
                    "Install a local Chrome/Chromium browser or run `python -m playwright install chromium`",
                ) from error
            context = browser.new_context(
                viewport={"width": 1440, "height": 900},
                device_scale_factor=1,
                java_script_enabled=False,
                locale="en-US",
                timezone_id="UTC",
                service_workers="block",
                accept_downloads=False,
            )
            context.set_offline(True)

            def block_request(route: Any) -> None:
                nonlocal blocked_requests
                blocked_requests += 1
                route.abort("blockedbyclient")

            context.route("**/*", block_request)
            page = context.new_page()
            try:
                page.set_content(guarded_html, wait_until="domcontentloaded", timeout=15_000)
                title = _clean_web_text(page.title(), 300)
                description = ""
                for selector in (
                    'meta[name="description"]',
                    'meta[property="og:description"]',
                    'meta[name="twitter:description"]',
                ):
                    locator = page.locator(selector)
                    candidate = (
                        locator.first.get_attribute("content") if locator.count() else None
                    )
                    description = _clean_web_text(candidate, 800)
                    if description:
                        break
                raw_points = page.locator(
                    "h1, h2, h3, p, li, button, [role=button]"
                ).all_text_contents()
                selling_points: list[str] = []
                seen_points: set[str] = set()
                for value in raw_points:
                    cleaned = _clean_web_text(value, 300)
                    key = cleaned.casefold()
                    if cleaned and key not in seen_points:
                        seen_points.add(key)
                        selling_points.append(cleaned)
                    if len(selling_points) == 24:
                        break
                colors: list[str] = []
                theme_locator = page.locator('meta[name="theme-color"]')
                theme_color = (
                    theme_locator.first.get_attribute("content")
                    if theme_locator.count()
                    else None
                )
                if isinstance(theme_color, str):
                    colors.append(theme_color)
                try:
                    computed = page.locator("body, h1, h2, h3, button, [role=button]").evaluate_all(
                        """elements => elements.flatMap(element => {
                            const style = getComputedStyle(element);
                            return [style.color, style.backgroundColor, style.borderColor];
                        })"""
                    )
                    if isinstance(computed, list):
                        colors.extend(value for value in computed if isinstance(value, str))
                except Exception:
                    # Screenshot capture remains useful when malformed page CSS
                    # prevents a computed-style observation.
                    pass
                brand_colors = _bounded_brand_colors(colors)
                page.screenshot(
                    path=str(destination),
                    type="png",
                    animations="disabled",
                    caret="hide",
                    full_page=False,
                    timeout=15_000,
                )
            except WorkerError:
                raise
            except Exception as error:
                destination.unlink(missing_ok=True)
                raise WorkerError(
                    "WEB_CAPTURE_RENDER_FAILED",
                    "Offline Chromium could not render the staged website snapshot",
                ) from error
            finally:
                context.close()
    finally:
        if browser is not None:
            try:
                browser.close()
            except Exception:
                pass
    width, height = _png_dimensions(destination)
    return {
        "screenshotPath": str(destination),
        "sourceUrl": source_url,
        "title": title,
        "description": description,
        "sellingPoints": selling_points,
        "brandColors": brand_colors,
        "width": width,
        "height": height,
        "publicAssetCount": len(asset_paths),
        "blockedRequestCount": blocked_requests,
        "networkAccess": "disabled",
        "javaScriptEnabled": False,
        "sandboxOrigin": "about:blank",
        "renderer": "isolated-offline-chromium-v1",
    }


def _clean_web_text(value: Any, maximum: int) -> str:
    if not isinstance(value, str):
        return ""
    printable = "".join(character if character.isprintable() else " " for character in value)
    return " ".join(printable.split())[:maximum]


def _bounded_brand_colors(values: list[str]) -> list[str]:
    accepted: list[str] = []
    seen: set[str] = set()
    pattern = re.compile(
        r"^(?:#[0-9a-fA-F]{3,8}|(?:rgb|rgba|hsl|hsla)\([^\r\n]{1,100}\)|transparent)$"
    )
    for value in values:
        normalized = value.strip()
        key = normalized.casefold()
        if pattern.fullmatch(normalized) and key not in seen:
            seen.add(key)
            accepted.append(normalized)
        if len(accepted) == 12:
            break
    return accepted


@contextmanager
def _open_renderer_page(
    *, editor_url: str, project_id: str, revision: int, viewport_width: int
):
    try:
        from playwright.sync_api import sync_playwright
    except ImportError as error:
        raise CapabilityUnavailable(
            "headless-renderer",
            "Install openchatcut-media-worker[render] and run `python -m playwright install chromium`",
        ) from error

    query = urlencode({"revision": revision, "width": min(viewport_width, 3840)})
    renderer_url = f"{editor_url}/render/{quote(project_id, safe='')}?{query}"
    with sync_playwright() as playwright:
        executable = _find_system_chromium()
        launch_options: dict[str, Any] = {
            "headless": True,
            "args": [
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-default-apps",
                "--disable-sync",
                "--metrics-recording-only",
                "--no-first-run",
                "--no-default-browser-check",
            ],
        }
        if executable is not None:
            launch_options["executable_path"] = str(executable)
        try:
            browser = playwright.chromium.launch(**launch_options)
        except Exception as error:
            raise CapabilityUnavailable(
                "headless-renderer",
                "Install a local Chrome/Chromium browser or run `python -m playwright install chromium`",
            ) from error
        context = browser.new_context(
            viewport={"width": min(viewport_width, 3840), "height": max(720, min(viewport_width, 3840))},
            device_scale_factor=1,
            locale="en-US",
            timezone_id="UTC",
            service_workers="block",
        )

        def route_request(route: Any) -> None:
            if _is_allowed_browser_url(route.request.url):
                route.continue_()
            else:
                route.abort("blockedbyclient")

        context.route("**/*", route_request)
        page = context.new_page()
        try:
            page.goto(renderer_url, wait_until="networkidle", timeout=60_000)
            page.wait_for_function(
                """() => {
                    const state = document.documentElement.dataset.openchatcutRendererState;
                    return state === 'ready' || state === 'error';
                }""",
                timeout=90_000,
            )
            state = page.evaluate("document.documentElement.dataset.openchatcutRendererState")
            if state != "ready":
                message = page.evaluate(
                    "document.documentElement.dataset.openchatcutRendererMessage || 'renderer initialization failed'"
                )
                raise WorkerError("HEADLESS_RENDER_FAILED", str(message)[:500])
            yield page
        except WorkerError:
            raise
        except Exception as error:
            raise WorkerError(
                "HEADLESS_RENDERER_UNREACHABLE",
                "The local Web renderer did not become ready",
            ) from error
        finally:
            try:
                context.close()
                browser.close()
            except Exception:
                pass


def _render_canvas_png(
    page: Any,
    *,
    time_ticks: int,
    document_hash: str,
    width: int | None = None,
    height: int | None = None,
) -> bytes:
    try:
        result = page.evaluate(
            """async ({time, width, height}) => {
                const rendered = await window.__OPENCHATCUT_RENDERER__.renderAt(time);
                const canvas = document.querySelector('canvas[data-openchatcut-render-canvas]');
                if (!width || !height || (canvas.width === width && canvas.height === height)) {
                    return { rendered, dataUrl: canvas.toDataURL('image/png') };
                }
                const scaled = document.createElement('canvas');
                scaled.width = width;
                scaled.height = height;
                const context = scaled.getContext('2d', { alpha: true });
                context.drawImage(canvas, 0, 0, width, height);
                return { rendered, dataUrl: scaled.toDataURL('image/png') };
            }""",
            {"time": time_ticks, "width": width, "height": height},
        )
    except Exception as error:
        raise WorkerError(
            "HEADLESS_RENDER_FAILED",
            f"Scene graph failed at timeline tick {time_ticks}",
        ) from error
    if result.get("rendered", {}).get("documentHash") != document_hash:
        raise WorkerError("PINNED_REVISION_MISMATCH", "Web renderer loaded a different revision")
    data_url = result.get("dataUrl")
    if not isinstance(data_url, str) or not data_url.startswith("data:image/png;base64,"):
        raise WorkerError("HEADLESS_RENDER_FAILED", "Renderer did not return a PNG frame")
    try:
        return base64.b64decode(data_url.partition(",")[2], validate=True)
    except ValueError as error:
        raise WorkerError("HEADLESS_RENDER_FAILED", "Renderer returned invalid PNG data") from error


def _atempo_chain(rate: float) -> list[str]:
    filters: list[str] = []
    remaining = rate
    while remaining > 2.0:
        filters.append("atempo=2")
        remaining /= 2.0
    while remaining < 0.5:
        filters.append("atempo=0.5")
        remaining /= 0.5
    filters.append(f"atempo={remaining:.9g}")
    return filters


def _install_atomic(*, temporary: Path, destination: Path, overwrite: bool) -> None:
    if overwrite:
        os.replace(temporary, destination)
        return
    try:
        os.link(temporary, destination)
    except FileExistsError as error:
        raise WorkerError(
            "OUTPUT_EXISTS",
            "The export output appeared while encoding and overwrite was not approved",
        ) from error
    finally:
        temporary.unlink(missing_ok=True)


def render_headless_export(
    *,
    data_root: Path,
    project_id: str,
    output_dir: Path,
    options: dict[str, Any],
    progress: Progress,
) -> dict[str, Any]:
    editor_url = _loopback_http_origin(options.get("editorUrl"))
    revision = options.get("revision")
    document_hash = options.get("documentHash")
    plan = options.get("plan")
    output_name = options.get("outputFileName")
    overwrite = options.get("allowOverwrite") is True
    if not isinstance(revision, int) or revision < 0:
        raise WorkerError("INVALID_EXPORT_PLAN", "Pinned revision is required")
    if not isinstance(document_hash, str) or not document_hash:
        raise WorkerError("INVALID_EXPORT_PLAN", "Pinned document hash is required")
    if not isinstance(plan, dict) or plan.get("renderer") != "headless-scene-graph-v1":
        raise WorkerError("INVALID_EXPORT_PLAN", "A validated scene-graph plan is required")
    if not isinstance(output_name, str):
        raise WorkerError("INVALID_OUTPUT_NAME", "Export outputFileName is required")
    destination = safe_output_path(output_dir=output_dir, file_name=output_name)
    if destination.exists() and not overwrite:
        raise WorkerError("OUTPUT_EXISTS", "The export output already exists")

    format_name = plan.get("format")
    width = plan.get("width")
    height = plan.get("height")
    duration_ticks = plan.get("durationTicks")
    timeline_start_ticks = plan.get("timelineStartTicks")
    ticks_per_second = plan.get("ticksPerSecond")
    fps = plan.get("fps")
    if (
        format_name not in ("mp4", "webm", "png", "png-sequence", "prores-4444")
        or not isinstance(width, int)
        or not 16 <= width <= 16_384
        or not isinstance(height, int)
        or not 16 <= height <= 16_384
        or not isinstance(duration_ticks, int)
        or duration_ticks <= 0
        or not isinstance(timeline_start_ticks, int)
        or timeline_start_ticks < 0
        or not isinstance(ticks_per_second, int)
        or ticks_per_second <= 0
        or not isinstance(fps, dict)
        or not isinstance(fps.get("numerator"), int)
        or not isinstance(fps.get("denominator"), int)
        or fps["numerator"] <= 0
        or fps["denominator"] <= 0
    ):
        raise WorkerError("INVALID_EXPORT_PLAN", "Scene-graph export plan fields are invalid")
    fps_numerator = fps["numerator"]
    fps_denominator = fps["denominator"]
    frame_denominator = ticks_per_second * fps_denominator
    frame_count = max(1, math.ceil(duration_ticks * fps_numerator / frame_denominator))
    if frame_count > 100_000:
        raise WorkerError("EXPORT_FRAME_LIMIT", "PNG/video export exceeds the 100000-frame safety limit")
    duration_seconds = duration_ticks / ticks_per_second

    audio_inputs: list[dict[str, Any]] = []
    raw_audio_inputs = options.get("audioInputs", [])
    if not isinstance(raw_audio_inputs, list) or len(raw_audio_inputs) > 256:
        raise WorkerError("INVALID_EXPORT_PLAN", "Audio input list is invalid")
    for value in raw_audio_inputs:
        if not isinstance(value, dict) or not isinstance(value.get("inputPath"), str):
            raise WorkerError("INVALID_EXPORT_PLAN", "Audio input is invalid")
        resolved = resolve_under_root(value=value["inputPath"], root=data_root)
        timing = {
            key: value.get(key)
            for key in (
                "timelineStartTicks",
                "sourceStartTicks",
                "durationTicks",
                "playbackRate",
                "gain",
                "fadeInTicks",
                "fadeOutTicks",
                "fadeCurve",
            )
        }
        if (
            any(not isinstance(timing[key], int) for key in ("timelineStartTicks", "sourceStartTicks", "durationTicks"))
            or timing["timelineStartTicks"] < 0
            or timing["sourceStartTicks"] < 0
            or timing["durationTicks"] <= 0
            or not isinstance(timing["playbackRate"], (int, float))
            or not 0.05 <= timing["playbackRate"] <= 16
            or not isinstance(timing["gain"], (int, float))
            or not 0 <= timing["gain"] <= 10
            or not isinstance(timing["fadeInTicks"], int)
            or not isinstance(timing["fadeOutTicks"], int)
            or timing["fadeInTicks"] < 0
            or timing["fadeOutTicks"] < 0
            or timing["fadeInTicks"] * 2 > timing["durationTicks"]
            or timing["fadeOutTicks"] * 2 > timing["durationTicks"]
            or timing["fadeCurve"] != "equalPower"
        ):
            raise WorkerError("INVALID_EXPORT_PLAN", "Audio timing is invalid")
        audio_inputs.append({"path": resolved, **timing})

    progress(0.04, "Loading pinned scene graph")
    video_encoding = None
    with _open_renderer_page(
        editor_url=editor_url,
        project_id=project_id,
        revision=revision,
        viewport_width=min(width, 3840),
    ) as page:
        if format_name == "png":
            png = _render_canvas_png(
                page,
                time_ticks=timeline_start_ticks,
                document_hash=document_hash,
                width=width,
                height=height,
            )
            temporary = destination.with_name(f".{destination.stem}.{uuid.uuid4().hex}.tmp.png")
            temporary.write_bytes(png)
            _install_atomic(temporary=temporary, destination=destination, overwrite=overwrite)
        elif format_name == "png-sequence":
            temporary = destination.with_name(
                f".{destination.stem}.{uuid.uuid4().hex}.tmp.zip"
            )
            try:
                with zipfile.ZipFile(
                    temporary,
                    mode="w",
                    compression=zipfile.ZIP_STORED,
                    allowZip64=True,
                ) as sequence:
                    sequence.writestr(
                        "sequence.json",
                        json.dumps(
                            {
                                "format": "openchatcut-png-sequence",
                                "version": 1,
                                "revision": revision,
                                "documentHash": document_hash,
                                "width": width,
                                "height": height,
                                "fps": fps,
                                "frameCount": frame_count,
                                "timelineStartTicks": timeline_start_ticks,
                                "ticksPerSecond": ticks_per_second,
                            },
                            separators=(",", ":"),
                        ),
                    )
                    for frame_index in range(frame_count):
                        time_ticks = timeline_start_ticks + (
                            frame_index * frame_denominator // fps_numerator
                        )
                        png = _render_canvas_png(
                            page,
                            time_ticks=time_ticks,
                            document_hash=document_hash,
                            width=width,
                            height=height,
                        )
                        sequence.writestr(f"frames/frame_{frame_index:06d}.png", png)
                        progress(
                            0.08 + ((frame_index + 1) / frame_count) * 0.84,
                            f"Writing PNG frame {frame_index + 1}/{frame_count}",
                        )
                _install_atomic(
                    temporary=temporary,
                    destination=destination,
                    overwrite=overwrite,
                )
            except BaseException:
                temporary.unlink(missing_ok=True)
                raise
        else:
            temporary = destination.with_name(
                f".{destination.stem}.{uuid.uuid4().hex}.tmp{destination.suffix}"
            )
            command = [
                "ffmpeg",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-framerate",
                f"{fps_numerator}/{fps_denominator}",
                "-f",
                "image2pipe",
                "-vcodec",
                "png",
                "-i",
                "pipe:0",
            ]
            for audio in audio_inputs:
                source_duration = audio["durationTicks"] / ticks_per_second * audio["playbackRate"]
                command.extend(
                    [
                        "-ss",
                        f"{audio['sourceStartTicks'] / ticks_per_second:.9f}",
                        "-t",
                        f"{source_duration:.9f}",
                        "-i",
                        str(audio["path"]),
                    ]
                )
            video_filter = f"scale={width}:{height}:flags=lanczos"
            if format_name == "prores-4444":
                video_filter += ",format=yuva444p10le"
            else:
                video_filter += ",format=yuv420p"
            command.extend(["-vf", video_filter])

            if audio_inputs:
                chains: list[str] = []
                labels: list[str] = []
                for index, audio in enumerate(audio_inputs, start=1):
                    timeline_duration = audio["durationTicks"] / ticks_per_second
                    delay_ms = round(audio["timelineStartTicks"] / ticks_per_second * 1000)
                    filters = _atempo_chain(float(audio["playbackRate"]))
                    fade_in = audio["fadeInTicks"] / ticks_per_second
                    fade_out = audio["fadeOutTicks"] / ticks_per_second
                    if fade_in > 0:
                        filters.append(f"afade=t=in:st=0:d={fade_in:.9f}:curve=qsin")
                    if fade_out > 0:
                        fade_start = max(0.0, timeline_duration - fade_out)
                        filters.append(
                            f"afade=t=out:st={fade_start:.9f}:d={fade_out:.9f}:curve=qsin"
                        )
                    filters.extend(
                        [
                            f"volume={float(audio['gain']):.9g}",
                            f"atrim=duration={timeline_duration:.9f}",
                            f"adelay={delay_ms}:all=1",
                        ]
                    )
                    label = f"a{index}"
                    chains.append(f"[{index}:a]{','.join(filters)}[{label}]")
                    labels.append(f"[{label}]")
                chains.append(
                    f"{''.join(labels)}amix=inputs={len(labels)}:duration=longest:normalize=0,"
                    f"atrim=duration={duration_seconds:.9f},aresample=48000[aout]"
                )
                command.extend(["-filter_complex", ";".join(chains), "-map", "0:v:0", "-map", "[aout]"])
            else:
                command.extend(["-map", "0:v:0", "-an"])

            if format_name == "mp4":
                video_encoding, video_encoder_args = h264_encoder_arguments(quality=18)
                command.extend(
                    [*video_encoder_args, "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]
                )
            elif format_name == "webm":
                command.extend(
                    ["-c:v", "libvpx-vp9", "-crf", "28", "-b:v", "0", "-c:a", "libopus", "-b:a", "160k"]
                )
            else:
                command.extend(
                    ["-c:v", "prores_ks", "-profile:v", "4", "-pix_fmt", "yuva444p10le", "-c:a", "pcm_s24le"]
                )
            command.extend(["-t", f"{duration_seconds:.9f}", str(temporary)])
            process = subprocess.Popen(
                command,
                stdin=subprocess.PIPE,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                env=sanitized_environment(),
            )
            assert process.stdin is not None
            assert process.stderr is not None
            try:
                for frame_index in range(frame_count):
                    time_ticks = timeline_start_ticks + (
                        frame_index * frame_denominator // fps_numerator
                    )
                    png = _render_canvas_png(
                        page, time_ticks=time_ticks, document_hash=document_hash
                    )
                    process.stdin.write(png)
                    progress(
                        0.08 + ((frame_index + 1) / frame_count) * 0.84,
                        f"Streaming frame {frame_index + 1}/{frame_count} to FFmpeg",
                    )
                process.stdin.close()
                stderr = process.stderr.read().decode("utf-8", errors="replace").strip()
                return_code = process.wait()
                if return_code != 0:
                    raise WorkerError(
                        "FFMPEG_EXPORT_FAILED",
                        stderr[-4_000:] or f"FFmpeg exited with status {return_code}",
                    )
                _install_atomic(
                    temporary=temporary, destination=destination, overwrite=overwrite
                )
            except Exception:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                temporary.unlink(missing_ok=True)
                raise

    digest = _sha256_file(destination)
    return {
        "outputPath": str(destination),
        "byteSize": destination.stat().st_size,
        "sha256": digest,
        "renderer": "headless-scene-graph-v1",
        "revision": revision,
        "documentHash": document_hash,
        "frameCount": 1 if format_name == "png" else frame_count,
        "audioSourceCount": len(audio_inputs),
        "videoEncoding": video_encoding,
    }


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def render_preview_frames(
    *,
    project_id: str,
    job_id: str,
    output_dir: Path,
    options: dict[str, Any],
    progress: Progress,
) -> dict[str, Any]:
    try:
        from playwright.sync_api import sync_playwright
    except ImportError as error:
        raise CapabilityUnavailable(
            "headless-preview",
            "Install openchatcut-media-worker[render] and run `python -m playwright install chromium`",
        ) from error

    editor_url = _loopback_http_origin(options.get("editorUrl"))
    revision = options.get("revision")
    document_hash = options.get("documentHash")
    times_ticks = options.get("timesTicks")
    preview_width = options.get("previewWidth", 1280)
    if not isinstance(revision, int) or revision < 0:
        raise WorkerError("INVALID_PREVIEW_REQUEST", "Pinned revision must be a non-negative integer")
    if not isinstance(document_hash, str) or not document_hash:
        raise WorkerError("INVALID_PREVIEW_REQUEST", "Pinned document hash is required")
    if (
        not isinstance(times_ticks, list)
        or not 1 <= len(times_ticks) <= 24
        or any(not isinstance(value, int) or value < 0 for value in times_ticks)
    ):
        raise WorkerError("INVALID_PREVIEW_REQUEST", "Preview times must contain 1 to 24 integer ticks")
    if not isinstance(preview_width, int) or not 64 <= preview_width <= 3840:
        raise WorkerError("INVALID_PREVIEW_REQUEST", "Preview width must be between 64 and 3840")

    query = urlencode({"revision": revision, "width": preview_width})
    renderer_url = f"{editor_url}/render/{quote(project_id, safe='')}?{query}"
    browser = None
    frames: list[dict[str, Any]] = []
    try:
        with sync_playwright() as playwright:
            executable = _find_system_chromium()
            launch_options: dict[str, Any] = {
                "headless": True,
                "args": [
                    "--disable-background-networking",
                    "--disable-component-update",
                    "--disable-default-apps",
                    "--disable-sync",
                    "--metrics-recording-only",
                    "--no-first-run",
                    "--no-default-browser-check",
                ],
            }
            if executable is not None:
                launch_options["executable_path"] = str(executable)
            try:
                browser = playwright.chromium.launch(**launch_options)
            except Exception as error:
                raise CapabilityUnavailable(
                    "headless-preview",
                    "Install a local Chrome/Chromium browser or run `python -m playwright install chromium`",
                ) from error
            context = browser.new_context(
                viewport={"width": preview_width, "height": max(720, preview_width)},
                device_scale_factor=1,
                locale="en-US",
                timezone_id="UTC",
                service_workers="block",
            )

            def route_request(route: Any) -> None:
                if _is_allowed_browser_url(route.request.url):
                    route.continue_()
                else:
                    route.abort("blockedbyclient")

            context.route("**/*", route_request)
            page = context.new_page()
            progress(0.08, "Loading pinned scene graph")
            try:
                page.goto(renderer_url, wait_until="networkidle", timeout=60_000)
                page.wait_for_function(
                    """() => {
                        const state = document.documentElement.dataset.openchatcutRendererState;
                        return state === 'ready' || state === 'error';
                    }""",
                    timeout=90_000,
                )
            except Exception as error:
                raise WorkerError(
                    "HEADLESS_RENDERER_UNREACHABLE",
                    "The local Web renderer did not become ready",
                ) from error
            state = page.evaluate("document.documentElement.dataset.openchatcutRendererState")
            if state != "ready":
                message = page.evaluate(
                    "document.documentElement.dataset.openchatcutRendererMessage || 'renderer initialization failed'"
                )
                raise WorkerError("HEADLESS_RENDER_FAILED", str(message)[:500])

            canvas = page.locator("canvas[data-openchatcut-render-canvas]")
            for index, time_ticks in enumerate(times_ticks):
                progress(
                    0.1 + (index / len(times_ticks)) * 0.82,
                    f"Rendering preview frame {index + 1}/{len(times_ticks)}",
                )
                try:
                    rendered = page.evaluate(
                        "time => window.__OPENCHATCUT_RENDERER__.renderAt(time)",
                        time_ticks,
                    )
                except Exception as error:
                    raise WorkerError(
                        "HEADLESS_RENDER_FAILED",
                        f"Scene graph failed at timeline tick {time_ticks}",
                    ) from error
                if rendered.get("documentHash") != document_hash:
                    raise WorkerError(
                        "PINNED_REVISION_MISMATCH",
                        "Web renderer loaded a different project revision",
                    )
                native_width = rendered.get("width")
                native_height = rendered.get("height")
                if not isinstance(native_width, int) or not isinstance(native_height, int):
                    raise WorkerError("HEADLESS_RENDER_FAILED", "Renderer returned invalid dimensions")
                display_height = max(1, round(preview_width * native_height / native_width))
                page.set_viewport_size(
                    {"width": preview_width, "height": min(max(display_height, 64), 16_384)}
                )
                destination = safe_output_path(
                    output_dir=output_dir,
                    file_name=f"{job_id}-{index:03}.png",
                )
                canvas.screenshot(path=str(destination), type="png", animations="disabled")
                width, height = _png_dimensions(destination)
                digest = hashlib.sha256(destination.read_bytes()).hexdigest()
                frames.append(
                    {
                        "path": str(destination),
                        "timeTicks": time_ticks,
                        "sha256": digest,
                        "byteSize": destination.stat().st_size,
                        "width": width,
                        "height": height,
                    }
                )
            context.close()
    finally:
        if browser is not None:
            try:
                browser.close()
            except Exception:
                # Leaving sync_playwright() already tears down its browser
                # transport. Cleanup must never mask a completed render or the
                # structured error that caused the context to exit.
                pass

    return {
        "renderer": "headless-scene-graph-v1",
        "projectId": project_id,
        "revision": revision,
        "documentHash": document_hash,
        "frames": frames,
    }

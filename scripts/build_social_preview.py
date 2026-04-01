#!/usr/bin/env python3

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs" / "assets" / "github-social-preview.png"
WIDTH = 1280
HEIGHT = 640


def load_font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    candidates = [
        "/usr/share/fonts/truetype/comfortaa/Comfortaa-Bold.ttf" if bold else "/usr/share/fonts/truetype/comfortaa/Comfortaa-Light.ttf",
        "/usr/share/fonts/truetype/lato/Lato-Bold.ttf" if bold else "/usr/share/fonts/truetype/lato/Lato-Medium.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf" if bold else "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]
    for candidate in candidates:
        path = Path(candidate)
        if path.exists():
            return ImageFont.truetype(str(path), size=size)
    return ImageFont.load_default()


def vertical_gradient(top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    image = Image.new("RGB", (WIDTH, HEIGHT), top)
    draw = ImageDraw.Draw(image)
    for y in range(HEIGHT):
        mix = y / max(HEIGHT - 1, 1)
        color = tuple(int(top[i] * (1.0 - mix) + bottom[i] * mix) for i in range(3))
        draw.line((0, y, WIDTH, y), fill=color)
    return image


def rounded_box(
    draw: ImageDraw.ImageDraw,
    box: tuple[int, int, int, int],
    fill: tuple[int, int, int, int],
    outline: tuple[int, int, int, int] | None = None,
    width: int = 1,
    radius: int = 24,
) -> None:
    draw.rounded_rectangle(box, radius=radius, fill=fill, outline=outline, width=width)


def add_background_layers(base: Image.Image) -> Image.Image:
    overlay = Image.new("RGBA", (WIDTH, HEIGHT), (0, 0, 0, 0))
    draw = ImageDraw.Draw(overlay)

    for offset, color in [
        ((-80, -40, 560, 420), (255, 255, 255, 90)),
        ((820, 70, 1320, 570), (255, 190, 92, 110)),
        ((940, -80, 1380, 320), (52, 130, 120, 110)),
    ]:
        draw.ellipse(offset, fill=color)

    for x in range(40, WIDTH, 72):
        draw.line((x, 0, x, HEIGHT), fill=(255, 255, 255, 20), width=1)
    for y in range(40, HEIGHT, 72):
        draw.line((0, y, WIDTH, y), fill=(255, 255, 255, 16), width=1)

    overlay = overlay.filter(ImageFilter.GaussianBlur(10))
    return Image.alpha_composite(base.convert("RGBA"), overlay)


def add_runtime_stack(base: Image.Image) -> None:
    draw = ImageDraw.Draw(base, "RGBA")
    label_font = load_font(26, bold=True)
    small_font = load_font(22, bold=False)

    cards = [
        ((840, 118, 1210, 206), "Planner", "mock | local | cloud", (28, 58, 56, 225)),
        ((790, 224, 1160, 312), "Skills", "YAML catalog + recovery", (33, 70, 67, 225)),
        ((840, 330, 1210, 418), "Gateway", "plan -> execute -> observe", (39, 83, 78, 225)),
        ((790, 436, 1160, 524), "Backends", "ROS2 | Gazebo | real robot", (47, 98, 92, 225)),
    ]

    shadow = Image.new("RGBA", base.size, (0, 0, 0, 0))
    shadow_draw = ImageDraw.Draw(shadow)
    for box, _, _, _ in cards:
        x0, y0, x1, y1 = box
        rounded_box(shadow_draw, (x0 + 12, y0 + 14, x1 + 12, y1 + 14), (8, 22, 24, 75), radius=28)
    shadow = shadow.filter(ImageFilter.GaussianBlur(12))
    base.alpha_composite(shadow)

    for box, title, subtitle, fill in cards:
        rounded_box(draw, box, fill, outline=(255, 255, 255, 58), width=2, radius=28)
        x0, y0, _, _ = box
        draw.text((x0 + 28, y0 + 18), title, font=label_font, fill=(245, 247, 243, 255))
        draw.text((x0 + 28, y0 + 50), subtitle, font=small_font, fill=(210, 225, 219, 255))

    for start, end in [
        ((1110, 206), (1110, 224)),
        ((1055, 312), (1055, 330)),
        ((1110, 418), (1110, 436)),
    ]:
        draw.line((start, end), fill=(255, 190, 92, 255), width=6)
        draw.polygon(
            [(end[0] - 10, end[1] - 2), (end[0] + 10, end[1] - 2), (end[0], end[1] + 14)],
            fill=(255, 190, 92, 255),
        )


def add_text(base: Image.Image) -> None:
    draw = ImageDraw.Draw(base, "RGBA")
    title_font = load_font(74, bold=True)
    subtitle_font = load_font(36, bold=True)
    body_font = load_font(26, bold=False)
    chip_font = load_font(24, bold=True)

    draw.text((72, 78), "roboclaw-rs", font=title_font, fill=(20, 34, 33, 255))
    draw.text((72, 170), "Agent-first robotics in Rust", font=subtitle_font, fill=(28, 58, 56, 255))

    body = [
        "ROS2 hooks, YAML skills, simulator and",
        "hardware backends, LLM-plannable execution,",
        "and one narrow runtime core.",
    ]
    y = 236
    for line in body:
        draw.text((72, y), line, font=body_font, fill=(55, 74, 73, 255))
        y += 36

    chips = [
        ("sim-first", (255, 190, 92, 255), (67, 49, 18, 255)),
        ("skill-based", (255, 240, 218, 255), (77, 60, 29, 255)),
        ("hardware abstraction", (216, 239, 233, 255), (24, 66, 61, 255)),
    ]
    x = 72
    for label, fill, text in chips:
        text_box = draw.textbbox((0, 0), label, font=chip_font)
        chip_width = text_box[2] - text_box[0] + 34
        rounded_box(draw, (x, 344, x + chip_width, 392), fill, radius=22)
        draw.text((x + 17, 355), label, font=chip_font, fill=text)
        x += chip_width + 16

    note_font = load_font(22, bold=False)
    notes = [
        "Planner -> PlanDecision",
        "SkillCatalog + recovery metadata",
        "Tool | RobotBackend | Ros2Bridge | Memory",
    ]
    y = 454
    for line in notes:
        draw.text((72, y), line, font=note_font, fill=(44, 66, 64, 255))
        y += 34


def main() -> None:
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    base = vertical_gradient((246, 241, 229), (196, 227, 220))
    image = add_background_layers(base)
    add_runtime_stack(image)
    add_text(image)
    image.convert("RGB").save(OUTPUT, format="PNG", optimize=True)
    print(OUTPUT)


if __name__ == "__main__":
    main()

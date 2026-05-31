#!/usr/bin/env python3
"""Genera el set de iconos de arrow (PNG) para Tauri en Linux.

Sin dependencias externas: usa SOLO la stdlib (zlib + struct). PIL no está
disponible en este entorno, así que rasterizamos a mano con matemática vectorial
(distancia a segmento) y codificamos el PNG nosotros.

arrow = visor de auditoría: el icono es una flecha "›" en verde de acento sobre
un cuadro redondeado oscuro (paleta githubDark de la UI).
"""
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(__file__), "icons")
os.makedirs(OUT, exist_ok=True)

BG = (13, 17, 23)        # fondo (githubDark-ish)
ACCENT = (63, 185, 80)   # verde de acento


def _dist_point_seg(px, py, ax, ay, bx, by):
    """Distancia del punto (px,py) al segmento (ax,ay)-(bx,by)."""
    dx, dy = bx - ax, by - ay
    if dx == 0 and dy == 0:
        return ((px - ax) ** 2 + (py - ay) ** 2) ** 0.5
    t = ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)
    t = max(0.0, min(1.0, t))
    cx, cy = ax + t * dx, ay + t * dy
    return ((px - cx) ** 2 + (py - cy) ** 2) ** 0.5


def render(size):
    """Devuelve un bytearray RGBA (size*size*4) con el icono rasterizado.

    Anti-aliasing barato: cobertura suave en el borde de cada forma (±1px).
    """
    s = float(size)
    radius = s * 0.22            # esquinas redondeadas del cuadro
    stroke = s * 0.085           # grosor de la flecha
    cx, cy = s * 0.46, s * 0.5   # centro de la flecha
    dx, dy = s * 0.17, s * 0.20  # extensión de la flecha
    # vértices del chevron ">": arriba-izq -> punta-der -> abajo-izq
    a = (cx - dx, cy - dy)
    tip = (cx + dx, cy)
    b = (cx - dx, cy + dy)
    half = stroke / 2.0

    buf = bytearray(size * size * 4)
    for y in range(size):
        for x in range(size):
            fx, fy = x + 0.5, y + 0.5
            # --- máscara del cuadro redondeado (rounded rect) ---
            # distancia firmada al borde interior con esquinas redondeadas
            qx = abs(fx - s / 2) - (s / 2 - radius)
            qy = abs(fy - s / 2) - (s / 2 - radius)
            qx_c, qy_c = max(qx, 0.0), max(qy, 0.0)
            outside = (qx_c ** 2 + qy_c ** 2) ** 0.5 - radius
            box_cov = max(0.0, min(1.0, 0.5 - outside))  # 1 dentro, 0 fuera
            if box_cov <= 0.0:
                continue  # transparente

            # color base = fondo
            r, g, bb = BG

            # --- flecha (dos segmentos) ---
            d = min(
                _dist_point_seg(fx, fy, a[0], a[1], tip[0], tip[1]),
                _dist_point_seg(fx, fy, tip[0], tip[1], b[0], b[1]),
            )
            arrow_cov = max(0.0, min(1.0, half - d + 0.5))
            if arrow_cov > 0.0:
                r = int(BG[0] + (ACCENT[0] - BG[0]) * arrow_cov)
                g = int(BG[1] + (ACCENT[1] - BG[1]) * arrow_cov)
                bb = int(BG[2] + (ACCENT[2] - BG[2]) * arrow_cov)

            i = (y * size + x) * 4
            buf[i] = r
            buf[i + 1] = g
            buf[i + 2] = bb
            buf[i + 3] = int(255 * box_cov)
    return buf


def _chunk(tag, data):
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    )


def write_png(path, size, rgba):
    # filtra cada scanline con filtro 0 (None) y comprime
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        start = y * size * 4
        raw += rgba[start:start + size * 4]
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8-bit RGBA
    png = (
        b"\x89PNG\r\n\x1a\n"
        + _chunk(b"IHDR", ihdr)
        + _chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + _chunk(b"IEND", b"")
    )
    with open(path, "wb") as f:
        f.write(png)
    print("wrote", os.path.basename(path), f"{size}x{size}")


def main():
    for size, name in [
        (32, "32x32.png"),
        (128, "128x128.png"),
        (256, "128x128@2x.png"),
        (512, "icon.png"),
    ]:
        write_png(os.path.join(OUT, name), size, render(size))


if __name__ == "__main__":
    main()

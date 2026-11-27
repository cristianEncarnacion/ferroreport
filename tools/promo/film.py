"""Films the race on the website into the 1920x1080 promo video, with its soundtrack.

Usage: python3 tools/promo/film.py http://localhost:8765/index.html out.mp4
Needs Playwright with Chrome or Chromium, ffmpeg and numpy. The page draws every frame as a pure
function of time (window.raceFrame and window.heatFrame in site/index.html), so the video is
exact and repeatable. The timeline is shared with tools/promo/sound.py.

docs/race.gif (the README's race) is the race scene of this video, cut with:
  ffmpeg -ss 2.6 -t 6.4 -i out.mp4 -vf "fps=12,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5" docs/race.gif
"""
import asyncio, os, subprocess, sys, tempfile

from playwright.async_api import async_playwright

URL, OUT = sys.argv[1], sys.argv[2]
FPS = 30
CHROME = os.environ.get("CHROME", "/usr/bin/google-chrome")
HERE = os.path.dirname(os.path.abspath(__file__))

SETUP = """() => {
  window.E = (x) => 1 - Math.pow(1 - Math.min(1, Math.max(0, x)), 3);
  const hero = document.querySelector('.hero');
  const card = document.createElement('div');
  card.className = 'endcard';
  card.innerHTML = '<div class="logo">ferro<span>report</span></div>' +
    '<div class="line">Same .jrxml. Same PDF. No JVM.</div>' +
    '<div class="url">Free and open source · cristianencarnacion.github.io/ferroreport</div>';
  hero.appendChild(card);
}"""

# Each scene: (seconds, script run for every frame with p = seconds into the scene, T = seconds into the video)
TITLE = """([p, T]) => {
  document.body.dataset.scene = '1';
  heatFrame(T, 0.12 * E(p / 2.6));
  const h = document.querySelector('.headline'), h1 = h.querySelector('h1'), hot = h.querySelector('.hot');
  const out = 1 - E((p - 2.3) / 0.3);
  h1.style.transform = `scale(${1.16 - 0.16 * E(p / 2.2)})`;
  h1.style.opacity = E(p / 0.45) * out;
  hot.style.display = 'inline-block';
  hot.style.transform = `translateY(${(1 - E((p - 0.72) / 0.45)) * 70}px)`;
  hot.style.opacity = E((p - 0.72) / 0.35);
  h.querySelector('.lead').style.opacity = E((p - 1.25) / 0.5) * out;
}"""
RACE = """([p, T]) => {
  document.body.dataset.scene = '2';
  raceFrame(p + 0.2);                      // the race itself starts 0.4 s into the scene
  heatFrame(T, 0.12 + 0.88 * E((p - 0.4) / 3.2));
  document.getElementById('race').style.opacity = E(p / 0.35);
  document.querySelector('header.top').style.opacity = E(p / 0.35);
}"""
END = """([p, T]) => {
  document.body.dataset.scene = '3';
  heatFrame(T, 1);
  const gone = 1 - E(p / 0.35);
  document.getElementById('race').style.opacity = gone;
  document.querySelector('header.top').style.opacity = gone;
  const card = document.querySelector('.endcard');
  card.style.opacity = E((p - 0.25) / 0.5);
  card.querySelector('.logo').style.transform = `scale(${1.22 - 0.22 * E((p - 0.25) / 0.9)})`;
}"""
SCENES = [(2.6, TITLE), (6.4, RACE), (3.2, END)]


async def scene(page, path, seconds, script, start):
    tmp = path + ".part.mp4"
    enc = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "image2pipe", "-framerate", str(FPS), "-c:v", "mjpeg", "-i", "-",
         "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p", "-r", str(FPS), tmp],
        stdin=subprocess.PIPE)
    for i in range(round(seconds * FPS)):
        await page.evaluate(script, [i / FPS, start + i / FPS])
        enc.stdin.write(await page.screenshot(type="jpeg", quality=92))
    enc.stdin.close()
    if enc.wait() != 0:
        raise SystemExit("ffmpeg failed on " + path)
    os.replace(tmp, path)


async def main():
    work = tempfile.mkdtemp()
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=CHROME)
        page = await browser.new_page(viewport={"width": 1280, "height": 720}, device_scale_factor=1.5)
        await page.goto(URL + "?film")
        await page.wait_for_timeout(1200)
        await page.evaluate(SETUP)
        await page.evaluate("raceFrame(0)")
        start = 0.0
        for i, (seconds, script) in enumerate(SCENES, start=1):
            await scene(page, f"{work}/{i}.mp4", seconds, script, start)
            start += seconds
        await browser.close()

    with open(f"{work}/list.txt", "w") as f:
        f.write("".join(f"file '{work}/{i}.mp4'\n" for i in range(1, len(SCENES) + 1)))
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", f"{work}/list.txt",
                    "-c", "copy", f"{work}/silent.mp4"], check=True)
    subprocess.run([sys.executable, os.path.join(HERE, "sound.py"), f"{work}/sound.wav"], check=True)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", f"{work}/silent.mp4", "-i", f"{work}/sound.wav",
                    "-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest",
                    "-movflags", "+faststart", OUT], check=True)
    print(OUT)


asyncio.run(main())

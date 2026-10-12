"""Exercise the built documentation at its Pages URL in desktop/mobile Chromium."""
import mimetypes
import os
from pathlib import Path
import sys
import tomllib
from urllib.parse import unquote, urlparse


def main():
    scratch = Path(os.environ.setdefault("TMPDIR", str(Path.home() / "tmp")))
    scratch.mkdir(parents=True, exist_ok=True)
    from playwright.sync_api import sync_playwright

    output = Path(sys.argv[1]).resolve()
    config = tomllib.loads((Path(__file__).parents[1] / "config.toml").read_text())
    base = config["base_url"].rstrip("/") + "/"
    origin = urlparse(base)
    pages = sorted(p for p in output.glob("*/index.html") if (p.parent.name not in {"tags", "categories"}))
    pages.insert(0, output / "index.html")
    if len(pages) < 2 or not all(p.is_file() for p in pages):
        raise SystemExit("No built guide pages found; run the build first")
    failures = []
    checks = 0

    def route_request(route):
        url = urlparse(route.request.url)
        if url.netloc != origin.netloc or not url.path.startswith(origin.path):
            route.abort()
            return
        path = (output / unquote(url.path[len(origin.path):])).resolve()
        if path.is_dir():
            path /= "index.html"
        if output not in path.parents or not path.is_file():
            failures.append(f"Missing browser resource: {route.request.url}")
            route.fulfill(status=404, body="Missing resource")
            return
        kind = mimetypes.guess_type(path)[0] or "application/octet-stream"
        route.fulfill(path=str(path), content_type=kind)

    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(args=["--disable-dev-shm-usage"])
        for width in (1440, 768, 390, 320):
            context = browser.new_context(viewport={"width": width, "height": 900}, reduced_motion="reduce", has_touch=width < 1024)
            context.route("**/*", route_request)
            page = context.new_page()
            page.on("pageerror", lambda error: failures.append(f"Browser script error: {error}"))
            for path in pages:
                url = base + path.relative_to(output).as_posix().removesuffix("index.html")
                response = page.goto(url, wait_until="networkidle")
                if response.status != 200:
                    failures.append(f"{width}: non-200 page {url}")
                if page.locator("main h1").count() != 1:
                    failures.append(f"{width}: expected one main heading {url}")
                overflow = page.evaluate("document.documentElement.scrollWidth > window.innerWidth")
                if overflow:
                    failures.append(f"{width}: horizontal document overflow {url}")
                # Native link navigation must be available in the mobile drawer.
                if width < 1024:
                    page.get_by_role("button", name="Open navigation", exact=True).focus()
                    toggle = page.locator("#navigation-toggle")
                    page.keyboard.press("Enter")
                    if toggle.get_attribute("aria-expanded") != "true":
                        failures.append(f"{width}: keyboard menu did not open {url}")
                    page.locator('#sidebar a[href$="/guides"], #sidebar a[href$="/guides/"]').first.wait_for(state="visible")
                    page.keyboard.press("Escape")
                    if toggle.get_attribute("aria-expanded") != "false":
                        failures.append(f"{width}: keyboard menu did not close {url}")
                checks += 1
            # Exercise navigation, not just link presence. The header must not
            # intercept the guide list, including on narrow touch viewports.
            page.goto(base + "guides/", wait_until="networkidle")
            github = page.locator('header a[href="https://github.com/imazen/zenbench"]')
            if github.count() != 1 or not github.is_visible():
                failures.append(f"{width}: GitHub link is not directly visible in the header")
            if width >= 1024:
                page.locator('.guide-toc a[href$="/getting-started/"]').click()
                page.wait_for_url(base + "getting-started/")
            for destination in ("getting-started/", "iterate/", "results/"):
                if width < 1024:
                    page.get_by_role("button", name="Open navigation", exact=True).click()
                link = page.locator(f'#sidebar a[href="{base}{destination}"]')
                try:
                    link.click(timeout=3000)
                    page.wait_for_url(base + destination, timeout=3000)
                except Exception as error:
                    failures.append(f"{width}: guide link {destination} cannot navigate: {error}")
                    page.goto(base + "guides/", wait_until="networkidle")
            page.goto(base + "guides/", wait_until="networkidle")
            page.locator('main a[href$="/getting-started/"]').first.click()
            page.wait_for_url(base + "getting-started/")
            # The theme's real copy button must preserve the displayed command.
            page.goto(base + "external-programs/", wait_until="networkidle")
            copy = page.locator("main .copy-code-button").first
            if not copy.is_visible():
                failures.append(f"{width}: no visible code-copy control")
            else:
                page.evaluate("""() => {
                    window.copiedText = null;
                    Object.defineProperty(navigator, 'clipboard', {
                        value: {writeText: async text => { window.copiedText = text; }},
                        configurable: true
                    });
                }""")
                expected = copy.locator("..").locator("code").inner_text()
                copy.click()
                if page.evaluate("window.copiedText") != expected:
                    failures.append(f"{width}: copied command differs from code block")
            context.close()
        browser.close()
    if failures:
        raise SystemExit("\n".join(sorted(set(failures))))
    print(f"Browser check: {checks} page/viewport visits; keyboard drawer, copy controls and script loading passed")


if __name__ == "__main__":
    main()

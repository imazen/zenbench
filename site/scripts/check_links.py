"""Check rendered HTML paths and fragments against the actual Pages output."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urljoin, urlparse
import sys
import tomllib


class Links(HTMLParser):
    def __init__(self, path):
        super().__init__(convert_charrefs=True)
        self.path = path
        self.ids = set()
        self.refs = []
        self.language = None
        self.viewport = False

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if tag == "html":
            self.language = attrs.get("lang")
        if tag == "meta" and attrs.get("name") == "viewport":
            self.viewport = True
        if "id" in attrs:
            self.ids.add(attrs["id"])
        for name in ("href", "src"):
            if name in attrs:
                self.refs.append(attrs[name])


def main():
    output = Path(sys.argv[1]).resolve()
    config = tomllib.loads((Path(__file__).parents[1] / "config.toml").read_text())
    base = config["base_url"].rstrip("/") + "/"
    origin = urlparse(base)
    prefix = origin.path.rstrip("/")
    docs = {}
    for path in output.rglob("*.html"):
        document = Links(path)
        document.feed(path.read_text())
        docs[path] = document
    if not docs:
        raise SystemExit("No built HTML found; run the build first")
    failures = []
    checked = 0
    for path, document in docs.items():
        # The retained standalone report has its own presentation/navigation.
        if path.name != "example-report.html":
            if document.language != "en" or not document.viewport:
                failures.append(f"{path.relative_to(output)}: missing English language/viewport")
        current = base + path.relative_to(output).as_posix()
        for ref in document.refs:
            target = urlparse(urljoin(current, ref))
            if (target.scheme, target.netloc) != (origin.scheme, origin.netloc):
                continue
            if target.path != prefix and not target.path.startswith(prefix + "/"):
                failures.append(f"{path.relative_to(output)}: link escapes Pages prefix: {ref}")
                continue
            relative = unquote(target.path[len(prefix) + 1:])
            resolved = (output / relative).resolve()
            if output != resolved and output not in resolved.parents:
                failures.append(f"{path.relative_to(output)}: path escapes output: {ref}")
                continue
            if resolved.is_dir():
                resolved /= "index.html"
            if not resolved.is_file():
                failures.append(f"{path.relative_to(output)}: missing target: {ref}")
                continue
            if target.fragment and resolved in docs:
                fragment = unquote(target.fragment)
                if fragment not in docs[resolved].ids:
                    failures.append(f"{path.relative_to(output)}: missing anchor: {ref}")
                    continue
            checked += 1
    if failures:
        raise SystemExit("\n".join(sorted(set(failures))))
    print(f"Rendered link check: {len(docs)} HTML files, {checked} local links/resources resolve")


if __name__ == "__main__":
    main()

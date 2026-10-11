#!/usr/bin/env python3
"""Add 800 accessible buttons to a copy of input.html for output-size checks."""

from pathlib import Path
import sys

source = Path(__file__).with_name("input.html").read_text()
bulk = '<section aria-label="Large test list">' + ''.join(
    f'<button type="button" aria-label="Bulk item {i:04d}">Bulk item {i:04d}</button>'
    for i in range(800)
) + '</section>'
Path(sys.argv[1]).write_text(source.replace("</html>", bulk + "</html>"))

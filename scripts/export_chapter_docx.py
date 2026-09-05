#!/usr/bin/env python3
"""Экспортирует научный текст главы 3 из Markdown в оформленный DOCX."""

from __future__ import annotations

import html
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "docs" / "chapter-3.md"
HTML_OUTPUT = ROOT / "docs" / "chapter-3-word.html"
DOCX_OUTPUT = ROOT / "docs" / "Глава 3. Разработка и исследование прототипа блокчейн-системы.docx"


def inline_markup(value: str) -> str:
    escaped = html.escape(value, quote=False)
    escaped = re.sub(
        r"`([^`]+)`",
        lambda match: f'<span class="code">{match.group(1)}</span>',
        escaped,
    )
    escaped = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", escaped)
    escaped = re.sub(r"\*([^*]+)\*", r"<em>\1</em>", escaped)
    return escaped


def component_figure() -> str:
    return """
<div class="figure">
  <table class="diagram top-flow">
    <tr>
      <td>Демонстрационный интерфейс<br><span class="small">React / TypeScript</span></td>
      <td class="arrow">→</td>
      <td>REST API<br><span class="small">Axum / Tokio</span></td>
      <td class="arrow">→</td>
      <td>Координатор сети<br><span class="small">раунд и маршрутизация</span></td>
    </tr>
  </table>
  <p class="diagram-arrow">↓</p>
  <table class="diagram consensus"><tr><td>RoundRobinQuorum: выбор автора → независимая проверка → подписанные голоса → кворум 3 из 4</td></tr></table>
  <p class="diagram-arrow">↓</p>
  <table class="diagram replicas">
    <tr><th colspan="4">Логически независимые реплики одного процесса Rust</th></tr>
    <tr>
      <td>Валидатор 1<br><span class="small">цепочка · состояние<br>пул · журнал</span></td>
      <td>Валидатор 2<br><span class="small">цепочка · состояние<br>пул · журнал</span></td>
      <td>Валидатор 3<br><span class="small">цепочка · состояние<br>пул · журнал</span></td>
      <td>Валидатор 4<br><span class="small">цепочка · состояние<br>пул · журнал</span></td>
    </tr>
  </table>
</div>
"""


def sequence_figure() -> str:
    rows = [
        ("1", "Пользователь", "REST API", "создание транзакции: отправитель, получатель, сумма"),
        ("2", "REST API", "Координатор", "канонизация, вычисление id и Ed25519-подпись"),
        ("3", "Координатор", "Активные валидаторы", "независимая проверка и помещение в локальные пулы"),
        ("4", "Пользователь", "REST API", "команда формирования блока"),
        ("5", "Координатор", "Автор раунда", "выбор по round mod 4"),
        ("6", "Автор раунда", "Кандидат блока", "transactions_root, переход состояния, state_hash, подпись"),
        ("7", "Координатор", "Активные валидаторы", "передача одинакового кандидата"),
        ("8", "Каждый валидатор", "Координатор", "проверка кандидата и подписанный голос либо отказ"),
        ("9", "Координатор", "Голоса", "проверка источников, уникальности, цели, раунда и подписей"),
        ("10", "Координатор", "Активные реплики", "при ≥ 3 голосах фиксация блока; иначе состояние не меняется"),
    ]
    rendered = "".join(
        f"<tr><td>{n}</td><td>{sender}</td><td>{receiver}</td><td>{action}</td></tr>"
        for n, sender, receiver, action in rows
    )
    return f"""
<div class="figure">
  <table class="sequence">
    <tr><th>Шаг</th><th>Источник</th><th>Получатель</th><th>Действие</th></tr>
    {rendered}
  </table>
</div>
"""


def render_table(lines: list[str]) -> str:
    rows: list[list[str]] = []
    for index, line in enumerate(lines):
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if index == 1 and all(re.fullmatch(r":?-{3,}:?", cell) for cell in cells):
            continue
        rows.append(cells)
    parts = ['<table class="data-table">']
    for row_index, cells in enumerate(rows):
        tag = "th" if row_index == 0 else "td"
        parts.append("<tr>")
        parts.extend(f"<{tag}>{inline_markup(cell)}</{tag}>" for cell in cells)
        parts.append("</tr>")
    parts.append("</table>")
    return "".join(parts)


def render_markdown(markdown: str) -> str:
    lines = markdown.splitlines()
    output: list[str] = []
    index = 0
    mermaid_index = 0

    while index < len(lines):
        line = lines[index]
        stripped = line.strip()
        if not stripped:
            index += 1
            continue

        if stripped.startswith("```"):
            language = stripped[3:].strip()
            index += 1
            block: list[str] = []
            while index < len(lines) and not lines[index].strip().startswith("```"):
                block.append(lines[index])
                index += 1
            index += 1
            if language == "mermaid":
                mermaid_index += 1
                output.append(component_figure() if mermaid_index == 1 else sequence_figure())
            else:
                output.append(f'<pre>{html.escape(chr(10).join(block))}</pre>')
            continue

        heading = re.match(r"^(#{1,4})\s+(.+)$", line)
        if heading:
            level = len(heading.group(1))
            output.append(f"<h{level}>{inline_markup(heading.group(2))}</h{level}>")
            index += 1
            continue

        if stripped.startswith("|"):
            table_lines: list[str] = []
            while index < len(lines) and lines[index].strip().startswith("|"):
                table_lines.append(lines[index])
                index += 1
            output.append(render_table(table_lines))
            continue

        if re.match(r"^-\s+", stripped):
            items: list[str] = []
            while index < len(lines) and re.match(r"^-\s+", lines[index].strip()):
                items.append(re.sub(r"^-\s+", "", lines[index].strip()))
                index += 1
            output.append("<ul>" + "".join(f"<li>{inline_markup(item)}</li>" for item in items) + "</ul>")
            continue

        if re.match(r"^\d+\.\s+", stripped):
            items: list[str] = []
            while index < len(lines) and re.match(r"^\d+\.\s+", lines[index].strip()):
                items.append(re.sub(r"^\d+\.\s+", "", lines[index].strip()))
                index += 1
            output.append("<ol>" + "".join(f"<li>{inline_markup(item)}</li>" for item in items) + "</ol>")
            continue

        paragraph = [stripped]
        index += 1
        while index < len(lines):
            candidate = lines[index].strip()
            if (
                not candidate
                or candidate.startswith("#")
                or candidate.startswith("```")
                or candidate.startswith("|")
                or re.match(r"^-\s+", candidate)
                or re.match(r"^\d+\.\s+", candidate)
            ):
                break
            paragraph.append(candidate)
            index += 1
        text = " ".join(paragraph)
        css_class = "caption" if text.startswith(("Рисунок ", "Таблица ")) else ""
        output.append(f'<p class="{css_class}">{inline_markup(text)}</p>')

    return "\n".join(output)


def build_html(body: str) -> str:
    return f"""<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<title>Глава 3. Разработка и исследование прототипа блокчейн-системы</title>
<style>
@page {{ size: A4; margin: 2cm 1.5cm 2cm 3cm; }}
body {{ font-family: "Times New Roman", serif; font-size: 14pt; line-height: 1.5; color: #000; }}
p {{ margin: 0 0 0.3em 0; text-indent: 1.25cm; text-align: justify; }}
h1 {{ font-size: 14pt; text-align: center; font-weight: bold; margin: 0 0 1.2em 0; page-break-before: always; }}
h2 {{ font-size: 14pt; font-weight: bold; margin: 1.2em 0 0.7em 0; page-break-after: avoid; }}
h3 {{ font-size: 14pt; font-weight: bold; margin: 1em 0 0.5em 0; page-break-after: avoid; }}
h4 {{ font-size: 14pt; font-weight: bold; font-style: italic; margin: 0.9em 0 0.4em 0; page-break-after: avoid; }}
.code {{ font-family: "Courier New", monospace; font-size: 11pt; }}
pre {{ font-family: "Courier New", monospace; font-size: 10pt; line-height: 1.15; border: 1px solid #777; padding: 8pt; white-space: pre-wrap; page-break-inside: avoid; }}
ul, ol {{ margin-top: 0.2em; margin-bottom: 0.5em; }}
li {{ margin-bottom: 0.15em; text-align: justify; }}
.caption {{ text-align: center; text-indent: 0; margin: 0.5em 0; page-break-after: avoid; }}
table {{ border-collapse: collapse; width: 100%; margin: 0.4em 0 0.8em 0; page-break-inside: avoid; }}
.data-table th, .data-table td, .sequence th, .sequence td {{ border: 1px solid #000; padding: 4pt; vertical-align: top; font-size: 10.5pt; line-height: 1.15; }}
.data-table th, .sequence th {{ text-align: center; font-weight: bold; }}
.diagram td, .diagram th {{ border: 1px solid #000; padding: 7pt; text-align: center; vertical-align: middle; font-size: 11pt; line-height: 1.15; }}
.diagram .arrow {{ border: none; width: 5%; font-size: 16pt; }}
.diagram-arrow {{ text-align: center; text-indent: 0; font-size: 16pt; margin: 0; }}
.small {{ font-size: 9.5pt; }}
.figure {{ margin: 0.7em 0; page-break-inside: avoid; }}
.consensus {{ width: 82%; margin-left: 9%; }}
</style>
</head>
<body>{body}</body>
</html>"""


def main() -> None:
    body = render_markdown(SOURCE.read_text(encoding="utf-8"))
    HTML_OUTPUT.write_text(build_html(body), encoding="utf-8")
    subprocess.run(
        [
            "textutil",
            "-convert",
            "docx",
            "-output",
            str(DOCX_OUTPUT),
            str(HTML_OUTPUT),
        ],
        check=True,
    )
    print(DOCX_OUTPUT)


if __name__ == "__main__":
    main()

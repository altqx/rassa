#!/usr/bin/env python3
"""Verify Rassa's public libass headers and shared-library exports against pinned libass."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Any


PINNED_LIBASS_COMMIT = "3087d2b2ffda76602a17f9b09d25cb8addc8d313"
LIBASS_REPOSITORY = "https://github.com/libass/libass.git"
PUBLIC_HEADERS = ("ass_types.h", "ass.h")
PUBLIC_RECORDS = {"ass_style", "ass_event", "ass_track", "ass_image"}


class CheckError(RuntimeError):
    pass


def run(command: list[str], *, cwd: Path | None = None) -> str:
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except FileNotFoundError as error:
        raise CheckError(f"required command is unavailable: {command[0]}") from error
    except subprocess.CalledProcessError as error:
        detail = error.stderr.strip() or error.stdout.strip()
        raise CheckError(f"command failed ({' '.join(command)}): {detail}") from error
    return result.stdout


def resolve_upstream(arguments: argparse.Namespace, temporary: Path) -> Path:
    configured = arguments.upstream or os.environ.get("RASSA_LIBASS_UPSTREAM")
    if configured:
        upstream = Path(configured).resolve()
        if not (upstream / "libass" / "ass.h").is_file():
            raise CheckError(f"not a libass source checkout: {upstream}")
    else:
        upstream = temporary / "libass"
        run(["git", "clone", "--quiet", "--no-checkout", LIBASS_REPOSITORY, str(upstream)])
        run(["git", "fetch", "--quiet", "--depth", "1", "origin", arguments.commit], cwd=upstream)
        run(["git", "checkout", "--quiet", "--detach", "FETCH_HEAD"], cwd=upstream)

    if arguments.allow_unpinned:
        return upstream
    try:
        actual = run(["git", "rev-parse", "HEAD^{commit}"], cwd=upstream).strip()
    except CheckError as error:
        raise CheckError(
            f"{upstream} is not a Git checkout; use --allow-unpinned only for an intentional snapshot"
        ) from error
    expected = run(["git", "rev-parse", f"{arguments.commit}^{{commit}}"], cwd=upstream).strip()
    if actual != expected:
        raise CheckError(
            f"upstream checkout is {actual}, expected {expected}; checkout the pin or use --allow-unpinned"
        )
    return upstream


def clang_ast(clang: str, include_directory: Path) -> dict[str, Any]:
    source = '#include "ass.h"\n'
    # Write a temp .c file; `run` has no stdin, so AST dumps stay deterministic.
    with tempfile.NamedTemporaryFile("w", suffix=".c", delete=False) as file:
        file.write(source)
        translation_unit = Path(file.name)
    try:
        output = run(
            [
                clang,
                "-Xclang",
                "-ast-dump=json",
                "-fsyntax-only",
                "-x",
                "c",
                "-I",
                str(include_directory),
                str(translation_unit),
            ]
        )
    finally:
        translation_unit.unlink(missing_ok=True)
    return json.loads(output)


def canonical_type(type_name: str) -> str:
    canonical = " ".join(type_name.split())
    # Strip Clang source locations from anonymous enums so checkout paths are not ABI.
    return re.sub(
        r"enum \(unnamed(?: enum)? at .*?:\d+:\d+\)",
        "enum (anonymous)",
        canonical,
    )


def integer_value(node: dict[str, Any]) -> str | None:
    if "value" in node:
        return str(node["value"])
    for child in node.get("inner", []):
        value = integer_value(child)
        if value is not None:
            return value
    return None


def enum_constants(node: dict[str, Any]) -> tuple[tuple[str, str], ...]:
    next_value = 0
    constants: list[tuple[str, str]] = []
    for child in node.get("inner", []):
        if child.get("kind") != "EnumConstantDecl":
            continue
        raw_value = integer_value(child)
        value = int(raw_value, 0) if raw_value is not None else next_value
        constants.append((child["name"], str(value)))
        next_value = value + 1
    return tuple(constants)


def ast_contract(ast: dict[str, Any]) -> dict[str, Any]:
    contract: dict[str, Any] = {
        "functions": {},
        "records": {},
        "enums": {},
        "typedefs": {},
    }
    enum_number = 0
    for node in ast.get("inner", []):
        kind = node.get("kind")
        name = node.get("name", "")
        if kind == "FunctionDecl" and name.startswith("ass_"):
            contract["functions"][name] = canonical_type(node["type"]["qualType"])
        elif kind == "RecordDecl" and name in PUBLIC_RECORDS and node.get("completeDefinition"):
            fields = tuple(
                (child.get("name", ""), canonical_type(child["type"]["qualType"]))
                for child in node.get("inner", [])
                if child.get("kind") == "FieldDecl"
            )
            contract["records"][name] = fields
        elif kind == "EnumDecl":
            constants = enum_constants(node)
            if constants and any(value[0].startswith(("ASS_", "YCBCR_", "IMAGE_TYPE_")) for value in constants):
                key = name or f"anonymous:{enum_number}:{constants[0][0]}"
                enum_number += 1
                contract["enums"][key] = constants
        elif kind == "TypedefDecl" and (
            name.startswith("ASS_") or name in {"ass_msg_callback"}
        ):
            contract["typedefs"][name] = canonical_type(node["type"]["qualType"])
    return contract


def integer_macros(clang: str, include_directory: Path) -> dict[str, str]:
    output = run(
        [
            clang,
            "-dM",
            "-E",
            "-x",
            "c",
            "-I",
            str(include_directory),
            str(include_directory / "ass.h"),
        ]
    )
    names = re.compile(
        r"^(?:LIBASS_VERSION|(?:V|H)ALIGN_[A-Z_]+|ASS_JUSTIFY_[A-Z_]+|FONT_(?:WEIGHT|SLANT|WIDTH)_[A-Z_]+)$"
    )
    macros: dict[str, str] = {}
    for line in output.splitlines():
        match = re.match(r"#define\s+(\w+)\s+(.+)$", line)
        if match and names.match(match.group(1)):
            macros[match.group(1)] = " ".join(match.group(2).split())
    return macros


def compare_contract(local: dict[str, Any], upstream: dict[str, Any]) -> list[str]:
    failures: list[str] = []
    for category in ("functions", "records", "enums", "typedefs", "macros"):
        local_values = local[category]
        upstream_values = upstream[category]
        missing = sorted(set(upstream_values) - set(local_values))
        extra = sorted(set(local_values) - set(upstream_values))
        changed = sorted(
            key
            for key in set(local_values) & set(upstream_values)
            if local_values[key] != upstream_values[key]
        )
        for key in missing:
            failures.append(f"{category}: missing {key}")
        for key in extra:
            failures.append(f"{category}: unexpected {key}")
        for key in changed:
            failures.append(
                f"{category}: {key} differs\n  upstream: {upstream_values[key]!r}\n  local:    {local_values[key]!r}"
            )
    return failures


def exported_symbols(library: Path) -> set[str]:
    if not library.is_file():
        raise CheckError(f"shared library does not exist: {library}")
    if shutil.which("nm"):
        output = run(["nm", "-D", "--defined-only", str(library)])
        return {
            line.split()[-1].split("@", 1)[0]
            for line in output.splitlines()
            if line.split() and line.split()[-1].split("@", 1)[0].startswith("ass_")
        }
    if shutil.which("llvm-nm"):
        output = run(["llvm-nm", "--defined-only", "--dynamic", str(library)])
        return {
            line.split()[-1].split("@", 1)[0]
            for line in output.splitlines()
            if line.split() and line.split()[-1].split("@", 1)[0].startswith("ass_")
        }
    raise CheckError("neither nm nor llvm-nm is available")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--upstream",
        type=Path,
        help="libass Git checkout (or RASSA_LIBASS_UPSTREAM); cloned at the pin when omitted",
    )
    parser.add_argument(
        "--commit",
        default=os.environ.get("RASSA_LIBASS_COMMIT", PINNED_LIBASS_COMMIT),
        help="required upstream commit (default: pinned libass master)",
    )
    parser.add_argument(
        "--allow-unpinned",
        action="store_true",
        help="compare an intentional non-pinned checkout/snapshot",
    )
    parser.add_argument(
        "--local-include",
        type=Path,
        default=Path("include/ass"),
        help="directory containing Rassa ass.h and ass_types.h",
    )
    parser.add_argument(
        "--library",
        type=Path,
        default=Path("target/release/libass.so"),
        help="built Rassa libass compatibility shared library",
    )
    parser.add_argument("--clang", default=os.environ.get("CLANG", "clang"))
    arguments = parser.parse_args()

    local_include = arguments.local_include.resolve()
    for header in PUBLIC_HEADERS:
        if not (local_include / header).is_file():
            raise CheckError(f"local public header is missing: {local_include / header}")

    with tempfile.TemporaryDirectory(prefix="rassa-libass-api-") as directory:
        upstream = resolve_upstream(arguments, Path(directory))
        upstream_include = upstream / "libass"
        local_contract = ast_contract(clang_ast(arguments.clang, local_include))
        upstream_contract = ast_contract(clang_ast(arguments.clang, upstream_include))
        local_contract["macros"] = integer_macros(arguments.clang, local_include)
        upstream_contract["macros"] = integer_macros(arguments.clang, upstream_include)

        failures = compare_contract(local_contract, upstream_contract)
        declarations = set(upstream_contract["functions"])
        exports = exported_symbols(arguments.library.resolve())
        for symbol in sorted(declarations - exports):
            failures.append(f"exports: declared public function is missing from library: {symbol}")
        for symbol in sorted(exports - declarations):
            failures.append(f"exports: undeclared ass_* symbol is exported by library: {symbol}")

        if failures:
            print("libass public API conformance failed:", file=sys.stderr)
            for failure in failures:
                print(f"- {failure}", file=sys.stderr)
            return 1

        revision = run(["git", "rev-parse", "HEAD"], cwd=upstream).strip() if (upstream / ".git").exists() else "snapshot"
        print(
            f"libass public API conformance ok: {len(declarations)} functions, "
            f"{len(local_contract['records'])} public records, upstream {revision}"
        )
        return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except CheckError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2) from error

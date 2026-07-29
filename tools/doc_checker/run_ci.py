#!/usr/bin/env python3
import subprocess
import os
import sys

def main():
    # Run the doc_checker tool
    # We run it inside tools/doc_checker to keep relative paths intact
    script_dir = os.path.dirname(os.path.abspath(__file__))
    
    # Run cargo run with --strict and --events
    result = subprocess.run(
        ["cargo", "run", "--", "--strict", "--events"],
        cwd=script_dir,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True
    )
    
    # Gather output
    stdout = result.stdout
    stderr = result.stderr
    exit_code = result.returncode

    # Print stdout and stderr so it appears in GHA logs
    print(stdout)
    if stderr:
        print(stderr, file=sys.stderr)

    # Prepare Markdown summary
    summary = []
    summary.append("# Documentation Checker Results\n")
    
    # Parse findings
    lines = stdout.splitlines()
    errors = [line for line in lines if line.startswith("error:")]
    warnings = [line for line in lines if line.startswith("warning:")]
    
    # Look for the last lines containing count summary
    summary_line = ""
    for line in reversed(lines):
        if "Documentation findings" in line:
            summary_line = line
            break
            
    if exit_code == 0:
        summary.append("## ✅ All Documentation Checks Passed!\n")
        if summary_line:
            summary.append(f"**{summary_line}**\n")
    else:
        summary.append("## ❌ Documentation Gaps Detected\n")
        if summary_line:
            summary.append(f"**{summary_line}**\n")
        summary.append("Please resolve the following documented gaps before merging this Pull Request.\n")
        
        if errors:
            summary.append("### Errors")
            summary.append("<details open>")
            summary.append("<summary>Click to expand errors</summary>\n")
            summary.append("```")
            for err in errors:
                summary.append(err)
            summary.append("```")
            summary.append("</details>\n")
            
        if warnings:
            summary.append("### Warnings")
            summary.append("<details>")
            summary.append("<summary>Click to expand warnings</summary>\n")
            summary.append("```")
            for warn in warnings:
                summary.append(warn)
            summary.append("```")
            summary.append("</details>\n")
            
    # Write to GitHub Step Summary if running in CI
    summary_text = "\n".join(summary)
    github_summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_summary_path:
        with open(github_summary_path, "w", encoding="utf-8") as f:
            f.write(summary_text)
            
    sys.exit(exit_code)

if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Append an explicit review of an original benchmark PNG after inspecting it."""
import argparse
from pathlib import Path

from computer_use_benchmark import review_capture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--case-id', required=True)
    parser.add_argument('--reviewer', required=True)
    parser.add_argument('--notes', required=True, help='Observed rendered content and relevant limits')
    parser.add_argument('--outcome', choices=('passed', 'failed'), required=True)
    args = parser.parse_args()
    result = review_capture(args.report, args.case_id, args.reviewer, args.notes, outcome=args.outcome)
    print({'core_automation_passed': result['core_automation_passed'], 'core_passed': result['core_passed'],
           'target_baseline_passed': result['target_baseline_passed']})


if __name__ == '__main__':
    main()

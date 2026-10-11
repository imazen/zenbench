"""Validate retained native evidence and summarize independent process runs.

No runs or rounds are discarded here. Presentation summaries distinguish
cross-process medians, raw mean intervals, and the engine's paired analysis.
"""
import argparse
import json
from pathlib import Path
import statistics


def analyze(packet):
    groups = {}
    refusals = 0
    for run in packet['runs']:
        cpus = run['cpu_set']
        if cpus in ['default', '0,2']:
            assert run['exit_code'] != 0
            expected = ('CPU placement refused: Asymmetric topology' if cpus == 'default'
                        else 'requested CPUs are not in one known class')
            assert expected in run['stderr']
            assert 'evidence' not in run
            refusals += 1
            continue
        assert run['exit_code'] == 0, run
        evidence = run['evidence']
        selected = (evidence['process_after'] if cpus in ['performance', 'efficiency']
                    else sorted(map(int, cpus.split(','))))
        assert evidence['process_after'] == selected
        assert all(mask and set(mask) <= set(selected)
                   for pair in evidence['worker_masks'] for mask in pair)
        result = evidence['result']
        placement = result['cpu_placement']
        assert placement['effective_cpus'] == selected
        assert placement['status'] == 'enforced_uniform'
        if 'process_widen_attempts' in evidence:
            assert placement['enforcement'] == 'job_affinity'
            attempts = evidence['process_widen_attempts']
            assert len(attempts) == 12
            assert all(a['actual_cpus'] == selected
                       and set(a['requested_cpus']) > set(selected) for a in attempts)
        if cpus in ['performance', 'efficiency']:
            assert placement['policy'] == cpus
            classes = placement['topology']['classes']
            ranks = [c['performance_rank'] for c in classes]
            assert all(rank is not None for rank in ranks)
            expected_rank = (max if cpus == 'performance' else min)(ranks)
            expected_cpus = sorted(cpu for c in classes
                                   if c['performance_rank'] == expected_rank for cpu in c['cpus'])
            assert selected == expected_cpus
        # Group evidence additionally records its calling-thread mask; the
        # suite's process evidence has no group-scoped thread observation.
        expected_group = dict(placement, thread_affinity_effective=selected)
        for comparison in result['comparisons']:
            assert comparison['cpu_placement'] == expected_group
            rounds = comparison['samples']
            assert len(rounds) == comparison['completed_rounds'] == 200
            n = len(comparison['benchmarks'])
            assert all(sorted(r['execution_order']) == list(range(n))
                       and len(r['elapsed_ns']) == len(r['compensated_ns']) == n
                       and r['iterations'] > 0 for r in rounds)
            assert len({tuple(r['execution_order']) for r in rounds}) > 1
            null = next(a[2] for a in comparison['analyses']
                        if a[:2] == ['identical-a', 'identical-b'])
            slow = next(a[2] for a in comparison['analyses']
                        if a[:2] == ['identical-a', 'double-work'])
            assert slow['significant'] and slow['ci_lower'] > 0
            small = next((a[2] for a in comparison['analyses']
                          if a[:2] == ['identical-a', 'ten-percent-more']), None)
            if n == 4:
                assert small is not None and small['significant'] and small['ci_lower'] > 0
            entry = groups.setdefault((cpus, comparison['group_name']), [])
            entry.append({
                'precise_timing_enabled': evidence.get('precise_timing_enabled'),
                'reference_qpc_median_ns': statistics.median(evidence['reference_integer_qpc_ns'])
                if evidence.get('reference_integer_qpc_ns') else None,
                'median_ns': comparison['benchmarks'][0]['summary']['median'],
                'mean_ns': comparison['benchmarks'][0]['summary']['mean'],
                'mad_ns': comparison['benchmarks'][0]['summary']['mad'],
                'null_change_pct': null['pct_change'],
                'null_reported_significant': null['significant'],
                'null_ci_excludes_zero': null['ci_lower'] > 0 or null['ci_upper'] < 0,
                'double_change_pct': slow['pct_change'],
                'ten_percent_change_pct': small['pct_change'] if small else None,
                'paired_outliers': null['n_outliers'],
                'paired_drift': null['drift_correlation'],
                'peak_rss_bytes': run['peak_rss_bytes'],
            })
    summary = []
    for (cpus, workload), runs in groups.items():
        medians = [r['median_ns'] for r in runs]
        center = statistics.median(medians)
        summary.append({
            'cpu_set': cpus, 'workload': workload, 'processes': len(runs),
            'median_of_process_medians_ns': center,
            'process_median_min_ns': min(medians), 'process_median_max_ns': max(medians),
            'process_median_range_pct': 100 * (max(medians) - min(medians)) / center,
            'null_change_min_pct': min(r['null_change_pct'] for r in runs),
            'null_change_max_pct': max(r['null_change_pct'] for r in runs),
            'null_significant_runs': sum(r['null_reported_significant'] for r in runs),
            'null_ci_excludes_zero_runs': sum(r['null_ci_excludes_zero'] for r in runs),
            'double_change_min_pct': min(r['double_change_pct'] for r in runs),
            'double_change_max_pct': max(r['double_change_pct'] for r in runs),
            'runs': runs,
        })
    return {'binary_sha256': packet['binary_sha256'], 'default_refusals': refusals,
            'memory_limit_bytes': packet['memory_limit_bytes'],
            'peak_job_commit_bytes': packet['peak_job_commit_bytes'], 'groups': summary}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--topology', type=Path, help='Independent native Windows SDK inventory')
    parser.add_argument('--independent-orders', action='store_true',
                        help='Require a distinct schedule for every successful process/group')
    args = parser.parse_args()
    packet = next(json.loads(line) for line in args.log.read_text().splitlines()
                  if line.startswith('{'))
    result = analyze(packet)
    if args.independent_orders:
        schedules = {}
        for run in packet['runs']:
            if run['exit_code'] != 0:
                continue
            for comparison in run['evidence']['result']['comparisons']:
                order = tuple(tuple(r['execution_order']) for r in comparison['samples'])
                schedules.setdefault(comparison['group_name'], []).append(order)
        for workload, orders in schedules.items():
            assert len(orders) == len(set(orders)), f'Repeated execution schedule: {workload}'
    if args.topology:
        topology = json.loads(args.topology.read_text())
        core_records = topology['core_records']
        high = max(c['efficiency_class'] for c in core_records)
        low = min(c['efficiency_class'] for c in core_records)
        for run in packet['runs']:
            policy = run['cpu_set']
            if policy in ['performance', 'efficiency']:
                rank = high if policy == 'performance' else low
                expected = sorted(cpu for c in core_records if c['efficiency_class'] == rank
                                  for cpu in c['affinity']['logical_ids_in_group'])
                assert run['evidence']['process_after'] == expected
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    for g in result['groups']:
        print(f"{g['cpu_set']:7} {g['workload']:7} processes={g['processes']} "
              f"median={g['median_of_process_medians_ns']:.2f}ns "
              f"range={g['process_median_range_pct']:.3f}% "
              f"null-significant={g['null_significant_runs']}/{g['processes']} "
              f"null-CI-excludes-zero={g['null_ci_excludes_zero_runs']}/{g['processes']}")


if __name__ == '__main__':
    main()

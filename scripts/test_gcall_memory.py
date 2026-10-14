"""Deterministic Linux memory sampler interpretation tests."""
import os
from types import SimpleNamespace
import unittest
from gcall_memory import parse_memory, summarise_memory


class Memory(unittest.TestCase):
    def test_rss_and_highwater_are_kib_not_virtual_size(self):
        m = parse_memory('VmSize: 999999 kB\nVmRSS: 2048 kB\nVmHWM: 4096 kB\n')
        self.assertEqual(m, {'rss_kib': 2048, 'hwm_kib': 4096})

    def test_missing_is_unknown(self):
        self.assertEqual(parse_memory('Name: members\n'), {'rss_kib': None, 'hwm_kib': None})
        self.assertIsNone(summarise_memory([])['rss_peak_sampled_kib'])

    def test_summary_preserves_highwater_and_change_without_claiming_leak(self):
        samples = [{'t': 5, 'memory': {'rss_kib': 100, 'hwm_kib': 150}},
                   {'t': 15, 'memory': {'rss_kib': 180, 'hwm_kib': 250}},
                   {'t': 25, 'memory': {'rss_kib': 160, 'hwm_kib': 250}}]
        r = summarise_memory(samples)
        self.assertEqual(r['rss_peak_sampled_kib'], 180)
        self.assertEqual(r['hwm_peak_observed_kib'], 250)
        self.assertEqual(r['rss_delta_kib'], 60)
        self.assertEqual(r['rss_endpoint_slope_kib_per_s'], 3)
        self.assertFalse(r['leak_established'])

    def test_local_sampler_and_exited_process(self):
        from gcall_bench import sample_local
        self.assertGreater(sample_local(os.getpid())['memory']['rss_kib'], 0)
        self.assertIsNone(sample_local(999999999))

    def test_harness_reports_host_and_aggregate_clients_separately(self):
        from gcall_bench import analyse
        a = SimpleNamespace(label='fixture', cmd='desktop', n=4, k=3, secs=30, host_args='')
        samples = []
        for t, rss in [(5, 100), (25, 200)]:
            samples.append({'t': t, 'ticks': t * 10, 'sys_total': t * 100, 'sys_idle': t * 90,
                            'memory': {'rss_kib': rss, 'hwm_kib': rss},
                            'members': {'t': t, 'ticks': t * 20,
                                        'memory': {'rss_kib': rss * 3, 'hwm_kib': rss * 3}}})
        r = analyse(a, samples, None, None, {}, 8)
        self.assertEqual(r['memory']['host']['rss_last_kib'], 200)
        self.assertEqual(r['memory']['simulated_clients_process_total']['rss_last_kib'], 600)
        self.assertEqual(r['simulated_clients_cpu_pct_of_core'], 20)
        self.assertEqual(len(r['memory_samples']), 2)

    def test_partial_missing_does_not_become_zero(self):
        r = summarise_memory([{'t': 1, 'memory': {'rss_kib': None, 'hwm_kib': None}},
                              {'t': 2, 'memory': {'rss_kib': 200, 'hwm_kib': 300}}])
        self.assertEqual(r['rss_first_kib'], 200)
        self.assertIsNone(r['rss_endpoint_slope_kib_per_s'])


if __name__ == '__main__':
    unittest.main()

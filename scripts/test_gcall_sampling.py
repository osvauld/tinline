"""Short diagnostic runs must not invent CPU/rates or divide by zero."""
import unittest
from types import SimpleNamespace
from gcall_bench import analyse


class Sampling(unittest.TestCase):
    def args(self):
        return SimpleNamespace(label='short', cmd='desktop', n=4, k=3, secs=12, host_args='')

    def sample(self, t):
        return {'t': t, 'ticks': 10, 'sys_total': 100, 'sys_idle': 70}

    def test_single_sample_retains_audio_summary_without_fake_cpu(self):
        r = analyse(self.args(), [self.sample(10)], {'t_ms': 10}, {'t_ms': 10}, {'received': 8}, 4)
        self.assertIsNone(r['host_cpu_pct_of_core'])
        self.assertIsNone(r['system_busy_pct'])
        self.assertIsNone(r['host_cpu_peak_10s_pct_of_core'])
        self.assertEqual(r['members']['received'], 8)
        self.assertNotIn('host_up_kbps_wire', r)
        self.assertIn('resource_warning', r)

    def test_no_surviving_samples_is_explicitly_missing(self):
        r = analyse(self.args(), [None], None, None, {'received': 8}, 4)
        self.assertIsNone(r['host_cpu_pct_of_core'])
        self.assertEqual(r['members']['received'], 8)
        self.assertIn('resource_warning', r)

"""Deterministic result interpretation guards, not codec/transport tests."""
import unittest
from gcall_metrics import audit_result, legacy_match


class Metrics(unittest.TestCase):
    def test_late_is_not_hidden(self):
        r = audit_result({'n': 32, 'members': {'m2e_n': 1380, 'late_onsets': 90, 'm2e_p95': 775.7}})
        self.assertTrue(r['percentiles_exclude_late'])
        self.assertEqual(r['matched_onset_observations'], 1470)
        self.assertAlmostEqual(r['late_fraction_of_matched'], 90 / 1470)
        self.assertFalse(r['service_capacity_established'])

    def test_missing_samples_are_not_zero_latency(self):
        r = audit_result({'n': 12, 'members': {'m2e_n': 0, 'm2e_p95': 0}})
        self.assertIsNone(r['conditional_p95_ms'])
        self.assertIsNone(r['late_fraction_of_matched'])

    def test_spurts_are_not_receiver_pair_denominator(self):
        r = audit_result({'n': 12, 'members': {'spurts': 27, 'm2e_n': 270}})
        self.assertIsNone(r['never_heard_fraction'])
        self.assertIn('receiver-pair', ' '.join(r['warnings']))

    def test_default_workload_not_one_expected_speaker(self):
        r = audit_result({'n': 32, 'members': {}})
        self.assertAlmostEqual(r['expected_speakers_nominal'], 0.2 + 31 / 32)
        self.assertEqual(r['host_duty_nominal'], 0.2)

    def test_controlled_schedule_not_reported_as_random_duty(self):
        r = audit_result({'n': 4, 'talk_pattern': 'overlap', 'controlled_host_silent': True, 'members': {}})
        self.assertEqual(r['host_duty_nominal'], 0)
        self.assertEqual(r['member_duty_nominal'], 5 / 6)
        self.assertAlmostEqual(r['expected_speakers_nominal'], 2.5)
        self.assertFalse(r['service_capacity_established'])

    def test_legacy_matching_can_attribute_old_audio_to_new_spurt(self):
        # Frame originating from spurt 100ms arrives after a new onset at 400ms.
        # Legacy rule has no frame origin: reports 100ms, not the real 400ms.
        self.assertEqual(legacy_match([100, 400], 500, 0), 400)
        self.assertNotEqual(500 - legacy_match([100, 400], 500, 0), 500 - 100)

    def test_matching_never_uses_future_or_repeats(self):
        self.assertEqual(legacy_match([100, 600], 500, 0), 100)
        self.assertIsNone(legacy_match([100, 600], 500, 100))
        self.assertIsNone(legacy_match([600], 500, 0))


if __name__ == '__main__':
    unittest.main()

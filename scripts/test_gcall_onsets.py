"""Deterministic offline trace-ledger tests, not native codec tests."""
import unittest
from gcall_onsets import analyse


def trace(extra=(), end=2000):
    return [('ready', 1, 1, 0, 0), ('ready', 2, 2, 0, 0),
            ('onset', 1, 1, 0, 100), ('send', 1, 1, 10, 120),
            ('speech_end', 1, 1, 0, 500), *extra,
            ('finish', 1, 1, 0, end), ('finish', 2, 2, 0, end)]


class Ledger(unittest.TestCase):
    def test_stages(self):
        r = analyse(trace([('receive', 1, 2, 10, 150), ('audible', 1, 2, 0, 200)]), 1000)
        p = r['opportunities'][0]
        self.assertEqual(p['onset_to_first_arrival_us'], 50)
        self.assertEqual(p['arrival_to_audible_proxy_us'], 50)
        self.assertFalse(p['audible_frame_attribution_verified'])

    def test_never_received_in_denominator(self):
        r = analyse(trace(), 1000)
        self.assertEqual(r['counts']['not_received_by_deadline'], 1)
        self.assertEqual(r['counts']['opportunities'], 1)

    def test_late_not_removed(self):
        r = analyse(trace([('receive', 1, 2, 10, 1500)], end=2500), 1000)
        p = r['opportunities'][0]
        self.assertEqual(p['status'], 'late_arrival')
        self.assertEqual(p['onset_to_first_arrival_us'], 1400)
        self.assertEqual(r['counts']['not_received_by_deadline'], 1)

    def test_tail_is_censored_not_passed(self):
        r = analyse(trace(end=800), 1000)
        self.assertEqual(r['opportunities'][0]['status'], 'tail_censored')
        self.assertEqual(r['counts']['tail_censored'], 1)
        self.assertEqual(r['counts']['not_received_by_deadline'], 0)

    def test_other_spurt_packet_does_not_match(self):
        r = analyse(trace([('send', 1, 1, 9, 50), ('receive', 1, 2, 9, 150)]), 1000)
        self.assertIsNone(r['opportunities'][0]['onset_to_first_arrival_us'])

    def test_no_audible_proxy_across_next_spurt(self):
        r = analyse(trace([('receive', 1, 2, 10, 150), ('onset', 1, 1, 0, 600),
                           ('audible', 1, 2, 0, 700)]), 1000)
        self.assertIsNone(r['opportunities'][0]['arrival_to_audible_proxy_us'])

    def test_late_join_not_opportunity(self):
        t = trace()
        t[1] = ('ready', 2, 2, 0, 300)
        self.assertEqual(analyse(t, 1000)['counts']['opportunities'], 0)

    def test_host_wait_uses_host_clock_only(self):
        t = trace([('send', 1, 1, 11, 140), ('receive', 1, 2, 11, 400)])
        host = [('host_receive', 1, 0, 10, 5000),
                ('host_receive', 1, 0, 11, 5200), ('forward_admit', 1, 0, 11, 5210)]
        p = analyse(t, 1000, host)['opportunities'][0]
        self.assertEqual(p['host_observed_admission_wait_us'], 210)
        self.assertEqual(p['onset_to_first_arrival_us'], 300)

    def test_host_admission_does_not_claim_delivery(self):
        p = analyse(trace(), 1000, [('host_receive', 1, 0, 10, 5000),
                                    ('forward_admit', 1, 0, 10, 5010)])['opportunities'][0]
        self.assertEqual(p['host_observed_admission_wait_us'], 10)
        self.assertEqual(p['status'], 'not_received_by_deadline')

    def test_missing_finish_is_error_not_success(self):
        with self.assertRaises(ValueError):
            analyse(trace()[:-1], 1000)


if __name__ == '__main__':
    unittest.main()

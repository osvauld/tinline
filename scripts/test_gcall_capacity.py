"""Tests for the capacity sweep's measurement/coordination guards."""
import unittest
from gcall_capacity import busy_reasons, validate_counts


class Guards(unittest.TestCase):
    def test_counts(self):
        self.assertEqual(validate_counts('12,16,24,32'), [12, 16, 24, 32])
        for bad in ('1', '65', '12,12', ''):
            with self.assertRaises(ValueError):
                validate_counts(bad)

    def test_idle(self):
        self.assertEqual(busy_reasons('cpu pressure 0%  load 0.5/8  free 14 GiB\ncompile                  free'), [])

    def test_compile_and_other_run(self):
        text = ('cpu pressure 10%  load 7.9/8  free 14 GiB\n'
                'compile                  search: cargo build\n'
                'run-1                    group-shards: year-bench')
        self.assertEqual(len(busy_reasons(text)), 4)

    def test_own_run_ignored_but_waiter_is_not(self):
        text = ('cpu pressure 0%  load 0.5/8  free 14 GiB\ncompile free\n'
                'run-0 capacity-measure: python\nwaiting for compile search-tests: cargo test')
        self.assertEqual(busy_reasons(text), ['waiting for compile search-tests: cargo test'])


if __name__ == '__main__':
    unittest.main()

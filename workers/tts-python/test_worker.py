import io
import struct
import unittest
import worker

class ProtocolTests(unittest.TestCase):
    def test_binary_frame_roundtrip(self):
        result = worker.encode_frame(1, 2, 0, True, [0.25, -0.5])
        header = struct.unpack('<4sQQIIIHB', result[:35])
        self.assertEqual(header, (b'PCM1', 1, 2, 0, 2, 24000, 1, 1))
        self.assertEqual(struct.unpack('<2f', result[35:]), (0.25, -0.5))

    def test_invalid_request_and_samples(self):
        good = dict(generation=1, utterance=2, text='Merhaba', voice='female_2')
        self.assertEqual(worker.validate_request(good), good)
        for change in [dict(text=''), dict(voice='/tmp/custom.wav'), dict(generation=-1), dict(generation=True), dict(unknown='command')]:
            with self.assertRaises(ValueError): worker.validate_request(good | change)
        for samples in [[], [float('nan')], [2.0], [0.] * 24001]:
            with self.assertRaises(ValueError): worker.encode_frame(1, 2, 0, False, samples)

if __name__ == '__main__': unittest.main()

import json
from pathlib import Path
import tempfile
import unittest
from urllib.error import HTTPError
from urllib.request import Request,ProxyHandler,build_opener

from .replay_summary import Replay,key


class ReplayTests(unittest.TestCase):
    def test_semantic_request_hash_still_compares_every_field(self):
        self.assertEqual(key({'a':1,'b':2}),key({'b':2,'a':1}))
        self.assertNotEqual(key({'model':'glm-5.3'}),key({'model':'glm-5.3-flash'}))
        self.assertNotEqual(key({'messages':[{'content':'old'}]}),key({'messages':[{'content':'new'}]}))

    def test_unknown_requests_cannot_fall_through_to_a_provider(self):
        with tempfile.TemporaryDirectory() as directory:
            body={'model':'glm-5.3','messages':[]}
            Path(directory,'response.json').write_text(json.dumps({'request':body,'response':{'choices':[]}}))
            with Replay(directory) as replay:
                opener=build_opener(ProxyHandler({}))
                url=f'http://127.0.0.1:{replay.server.server_port}/v1/chat/completions'
                with opener.open(Request(url,data=json.dumps(body).encode())) as response:
                    self.assertEqual(response.status,200)
                with self.assertRaises(HTTPError) as error:
                    opener.open(Request(url,data=json.dumps({**body,'model':'changed'}).encode()))
                self.assertEqual(error.exception.code,409)
                self.assertEqual((replay.hits,replay.misses),(1,1))


if __name__=='__main__':unittest.main()

import importlib.util
import json
from pathlib import Path
import sys
import unittest

MODULE=Path(__file__).parents[1]/"src"/"windows_process.py"
sys.path.insert(0,str(Path(__file__).parents[3]))
SPEC=importlib.util.spec_from_file_location("kyberia_windows_process",MODULE)
MODULE_OBJECT=importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(MODULE_OBJECT)

class SelectionTests(unittest.TestCase):
    def test_fixed_catalog_accepts_only_named_operation(self):
        command,timeout,limit=MODULE_OBJECT.select(json.dumps({"schemaVersion":1,"suite":"foundation","selector":"default","timeoutSeconds":3,"outputBytes":1024}).encode())
        self.assertEqual(command[1],["tools/dev.py","check"]); self.assertEqual((timeout,limit),(3,1024))
    def test_command_and_unknown_fields_cannot_be_injected(self):
        with self.assertRaises(ValueError): MODULE_OBJECT.select(json.dumps({"schemaVersion":1,"suite":"foundation","selector":"default","timeoutSeconds":3,"outputBytes":1024,"executable":"calc.exe"}).encode())
        with self.assertRaises(ValueError): MODULE_OBJECT.select(json.dumps({"schemaVersion":1,"suite":"evil","selector":"default","timeoutSeconds":3,"outputBytes":1024}).encode())
    def test_bounds(self):
        with self.assertRaises(ValueError): MODULE_OBJECT.select(json.dumps({"schemaVersion":1,"suite":"foundation","selector":"default","timeoutSeconds":0,"outputBytes":1024}).encode())
        with self.assertRaises(ValueError): MODULE_OBJECT.select(b"x"*(MODULE_OBJECT.MAX_REQUEST+1))

if __name__=="__main__": unittest.main()

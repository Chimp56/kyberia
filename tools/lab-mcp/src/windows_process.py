"""One-shot Windows Job Object supervisor with a fixed Kyberia catalog."""
import json, os, subprocess, sys
from research.active.process import _attach_windows_job, _close_windows_job

MAX_REQUEST=4096; MAX_OUTPUT=4_194_304; MAX_TIMEOUT=7200
COMMANDS={
    "foundation:default": (sys.executable,["tools/dev.py","check"]),
    "capture:default": (sys.executable,["tools/dev.py","test"]),
    "probe-wifi:default": (sys.executable,["-m","unittest","tests.test_windows_collector"]),
    "probe-kismet:expected_version=2025.01": (sys.executable,["-m","unittest","tests.test_kismet_live"]),
    "probe-kismet:fixture_set=golden-v1": (sys.executable,["-m","unittest","tests.test_kismet_database"]),
    "probe-sionna:cpu_or_gpu=cpu&scene_set=canonical-v1": (sys.executable,["workers/sionna/acceptance.py"]),
    "probe-spectrum:default": (sys.executable,["-m","unittest","tests.test_spectrum_probe"]),
}
def select(raw):
    if len(raw)>MAX_REQUEST: raise ValueError("request too large")
    request=json.loads(raw); expected={"schemaVersion","suite","selector","timeoutSeconds","outputBytes"}
    if type(request) is not dict or set(request)!=expected or request["schemaVersion"]!=1: raise ValueError("invalid request")
    command=COMMANDS.get(f'{request["suite"]}:{request["selector"]}')
    if command is None: raise ValueError("operation not in fixed Windows catalog")
    timeout=request["timeoutSeconds"]; output_limit=request["outputBytes"]
    if type(timeout) is not int or not 1<=timeout<=MAX_TIMEOUT or type(output_limit) is not int or not 1024<=output_limit<=MAX_OUTPUT: raise ValueError("limits rejected")
    return command,timeout,output_limit
def main():
    if os.name!="nt": raise RuntimeError("Windows containment helper requires Windows")
    command,timeout,output_limit=select(sys.stdin.buffer.read(MAX_REQUEST+1)); child=None; job=None; cleanup=[]
    try:
        child=subprocess.Popen([command[0],*command[1]],stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,shell=False,creationflags=0x4,env={"PATH":os.environ.get("PATH","")})
        job=_attach_windows_job(child)
        try: out,err=child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired: job.terminate(); out,err=child.communicate(); raise TimeoutError("command timed out")
        if len(out)+len(err)>output_limit: job.terminate(); raise ValueError("command output exceeded limit")
        sys.stdout.buffer.write(out); sys.stderr.buffer.write(err); return child.returncode
    finally:
        if job is not None: _close_windows_job(job,cleanup)
        if cleanup: raise OSError("Windows Job Object cleanup failed")
if __name__=="__main__": raise SystemExit(main())

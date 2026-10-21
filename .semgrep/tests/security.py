import subprocess
import torch

# ruleid: unrestricted-pytorch-load
torch.load("synthetic.pt", weights_only=False)
# ok: unrestricted-pytorch-load
torch.load("synthetic.pt", weights_only=True)
# ruleid: shell-subprocess
subprocess.run("synthetic", shell=True)
# ok: shell-subprocess
subprocess.run(["synthetic"], shell=False)

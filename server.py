"""Local server for the PCL2 and PCLCE luck tool.

It serves the static page and exposes a localhost-only endpoint that reproduces
PCL2's displayed Identify value from the values persisted in the registry.
"""

from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit
import hashlib
import json
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parent
PORT = 4173
MASK_64 = (1 << 64) - 1
HASH_XOR = 0xA98F501BC684032F


def read_registry_value(root, path, name):
    import winreg

    try:
        with winreg.OpenKey(root, path, 0, winreg.KEY_READ) as key:
            value, _ = winreg.QueryValueEx(key, name)
            return str(value)
    except (FileNotFoundError, OSError):
        return ""


def stable_hash(value):
    """Match MeloongCore's 64-bit UTF-16 stable hash used by PCL2."""
    result = 5381
    encoded = value.encode("utf-16-le")
    for offset in range(0, len(encoded), 2):
        char_code = int.from_bytes(encoded[offset:offset + 2], "little")
        result = ((result << 5) ^ result ^ char_code) & MASK_64
    return result ^ HASH_XOR


def format_pcl_identify(last_config, identify_seed):
    # PCL uppercases LastConfig and trims the leading/trailing braces before
    # concatenating it with the persisted Identify seed.
    normalized_config = last_config.upper().strip("{}")
    value = stable_hash(normalized_config + identify_seed)
    hex_value = f"{value:016X}"
    return "-".join((hex_value[4:8], hex_value[12:16], hex_value[0:4], hex_value[8:12]))


def read_pcl_identify():
    if sys.platform != "win32":
        return {
            "available": False,
            "message": "PCL2 registry lookup is only available on Windows.",
        }

    import winreg

    # Official PCL2 uses PCL. PCL's open-source debug branch uses PCLDebug.
    for folder in ("PCL", "PCLDebug"):
        identify_seed = read_registry_value(
            winreg.HKEY_CURRENT_USER,
            f"Software\\{folder}",
            "Identify",
        )
        last_config = read_registry_value(
            winreg.HKEY_LOCAL_MACHINE,
            r"SYSTEM\HardwareConfig",
            "LastConfig",
        )
        if len(identify_seed) >= 3 and last_config:
            return {
                "available": True,
                "identify": format_pcl_identify(last_config, identify_seed),
                "source": f"HKCU\\Software\\{folder}\\Identify + HKLM\\SYSTEM\\HardwareConfig\\LastConfig",
            }

    return {
        "available": False,
        "message": "PCL2 Identify data was not found. Launch PCL2 once, then reload this page.",
    }


def read_pclce_hardware():
    if sys.platform != "win32":
        return None

    powershell = shutil.which("powershell.exe") or shutil.which("powershell")
    if not powershell:
        return None

    script = r"""
$ErrorActionPreference = 'Stop'
function Get-FirstWmiValue($className, $propertyName) {
    foreach ($item in (Get-CimInstance -ClassName $className)) {
        if ($null -ne $item.$propertyName) {
            return ([string]$item.$propertyName).Trim()
        }
    }
    return ''
}
[pscustomobject]@{
    UUID = Get-FirstWmiValue 'Win32_ComputerSystemProduct' 'UUID'
    MB_Prod = Get-FirstWmiValue 'Win32_BaseBoard' 'Product'
    MB_SN = Get-FirstWmiValue 'Win32_BaseBoard' 'SerialNumber'
    CPU = Get-FirstWmiValue 'Win32_Processor' 'ProcessorId'
} | ConvertTo-Json -Compress
"""
    try:
        result = subprocess.run(
            [powershell, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script],
            capture_output=True,
            text=True,
            timeout=8,
            check=False,
        )
        if result.returncode != 0 or not result.stdout.strip():
            return None
        data = json.loads(result.stdout)
        return {key: str(data.get(key, "") or "").strip() for key in ("UUID", "MB_Prod", "MB_SN", "CPU")}
    except (OSError, subprocess.SubprocessError, ValueError, TypeError):
        return None


def format_pclce_identify(hardware):
    raw = (
        f"UUID:{hardware['UUID']}"
        f"|MB_Prod:{hardware['MB_Prod']}"
        f"|MB_SN:{hardware['MB_SN']}"
        f"|CPU:{hardware['CPU']}"
    )
    raw_hash = hashlib.sha512(raw.encode("utf-8")).hexdigest()
    sample = hashlib.sha512(f"PCL-CE|{raw_hash}|LauncherId".encode("utf-8")).hexdigest()
    hex_value = sample[64:80].upper()
    return "-".join((hex_value[0:4], hex_value[4:8], hex_value[8:12], hex_value[12:16]))


def read_pclce_identify():
    hardware = read_pclce_hardware()
    if not hardware or not any(hardware.values()):
        return {
            "available": False,
            "message": "PCLCE hardware identification was not available.",
        }
    return {
        "available": True,
        "identify": format_pclce_identify(hardware),
        "source": "WMI Win32_ComputerSystemProduct + Win32_BaseBoard + Win32_Processor",
    }


def read_identifiers():
    return {
        "pcl": read_pcl_identify(),
        "pclce": read_pclce_identify(),
    }


class RequestHandler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(ROOT), **kwargs)

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/api/identifiers":
            payload = json.dumps(read_identifiers(), ensure_ascii=True).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        if path == "/api/pcl-identify":
            payload = json.dumps(read_pcl_identify(), ensure_ascii=True).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        if path == "/api/pclce-identify":
            payload = json.dumps(read_pclce_identify(), ensure_ascii=True).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        super().do_GET()


if __name__ == "__main__":
    server = ThreadingHTTPServer(("127.0.0.1", PORT), RequestHandler)
    print(f"Serving {ROOT} at http://127.0.0.1:{PORT}/")
    print("Identifier endpoint: http://127.0.0.1:4173/api/identifiers")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()

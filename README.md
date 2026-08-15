# 今日人品 · PCL2 预测工具

这是一个同时复刻 PCL2 与 PCLCE 今日人品算法的网页版工具。网页本身仍然是纯前端；使用 `server.py` 启动时，还会在本机读取两套算法所需的识别信息。

```powershell
python server.py
```

也可以双击 `run.bat` 启动；关闭命令行窗口后服务会停止。然后打开 `http://localhost:4173/`。如果直接打开 `index.html`，网页无法读取 Windows 注册表，会回退到手动设备标识。

## 算法来源

PCL2 公开仓库会把百宝箱实现剥离，但仓库中的 `最新正式版.zip` 包含历史正式版二进制。对其中 `PageOtherTest.Jrrp()` 的 IL 进行核对后，得到以下规则：

### PCL2

1. PCL 先读取 `HKLM\\SYSTEM\\HardwareConfig\\LastConfig`，转为大写并去掉首尾大括号，再与 `HKCU\\Software\\PCL\\Identify` 拼接生成最终识别码。
2. 使用最终识别码、年、日序和日期中的日拼接今日人品的两个种子字符串。
3. 使用 MeloongCore 的 `GetStableHashCode`：初始值为 `5381`，逐个 UTF-16 字符执行 `((hash << 5) ^ hash ^ char)`，最后异或 `0xA98F501BC684032F`。
4. 两个哈希分别除以 `3`，相加后除以 `527`，取绝对值并对 `1001` 取余，再使用 .NET `Math.Round` 的银行家舍入。
5. 中间值不小于 `970` 时为 `100`；否则为 `RoundEven(value / 969 * 99)`。

### PCLCE

PCLCE 从 WMI 读取 `Win32_ComputerSystemProduct.UUID`、`Win32_BaseBoard.Product`、`Win32_BaseBoard.SerialNumber` 和 `Win32_Processor.ProcessorId`，拼成 `UUID:...|MB_Prod:...|MB_SN:...|CPU:...`。它先对该字符串做 SHA-512，再对 `PCL-CE|原哈希|LauncherId` 做 SHA-512，取第二次哈希的第 65 至 80 个十六进制字符并格式化为识别码。每日人品为 `DJB2Hash(yyyyMMdd + LauncherId)` 后使用 `.NET Random(seed).Next(0, 101)`。

网页用 JavaScript `BigInt` 复刻 PCL2 的 64 位哈希、银行家舍入和 PCLCE 的 .NET `Random`。`server.py` 只监听 `127.0.0.1`，不会把识别信息发送到远程服务器。如果 PCL2 尚未保存识别码或 WMI 不可用，对应算法可以继续手动填写识别码。

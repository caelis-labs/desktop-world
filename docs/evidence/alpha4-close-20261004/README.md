# alpha.4 Close 测试收尾

alpha.3 main 的三平台 CI 通过，但 tag CI 的 Windows job 在 TestCloseWithPendingNativeCall 返回 nil，见 run 37187275301。该测试原有的 20 ms 步骤期限可能在 native dispatch 前到期，导致没有 pending call；Close 返回 nil 是合法结果。这个测试假设早于 P2。

改为 fixture Before callback 发出 entered 信号，确认真实 fake-native 调用已经阻塞，再调用 Close 并要求 close_incomplete；最后释放 provider，等待执行返回并验证完整关闭成功。所有等待都有上限，失败也释放阻塞。不改生产实现。

本机 race 重复 100 次通过（stress.txt）；完整 scripts/check.sh 通过（check.txt）。Windows 验证仍指 CI 契约测试，并非真实电脑操作。alpha.3/tag/assets 不覆写，alpha.4 从新 clean HEAD 打包、重新验证版本/签名/校验值及 macOS package helper 场景。

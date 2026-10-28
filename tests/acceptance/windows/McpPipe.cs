// Bounded, Windows-compatible stdio transport. Does not send desktop input.
using System;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace DtwWindowsSmoke {
    public sealed class McpPipe {
        readonly Process process = new Process();
        readonly BlockingCollection<string> messages = new BlockingCollection<string>(4);
        readonly Thread stdout, stderr;
        string failure;
        public int Id { get { return process.Id; } }
        public static string QuoteArgument(string value) {
            StringBuilder quoted = new StringBuilder("\"");
            int slashes = 0;
            foreach (char c in value) {
                if (c == '\\') { slashes++; continue; }
                quoted.Append('\\', c == '"' ? slashes * 2 + 1 : slashes);
                quoted.Append(c); slashes = 0;
            }
            quoted.Append('\\', slashes * 2); quoted.Append('"');
            return quoted.ToString();
        }
        void Fail(string message) { Interlocked.CompareExchange(ref failure, message, null); }
        public McpPipe(string executable, string directory) {
            process.StartInfo = new ProcessStartInfo(executable, "mcp") {
                UseShellExecute = false, CreateNoWindow = true,
                RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
                StandardOutputEncoding = new UTF8Encoding(false), StandardErrorEncoding = new UTF8Encoding(false)
            };
            process.Start();
            try {
                stdout = new Thread(delegate() { PumpOutput(Path.Combine(directory, "stdout.jsonl")); });
                stderr = new Thread(delegate() { PumpError(Path.Combine(directory, "stderr.log")); });
                stdout.IsBackground = stderr.IsBackground = true;
                stdout.Start(); stderr.Start();
            } catch {
                try { if (!process.HasExited) { process.Kill(); process.WaitForExit(1000); } } catch { }
                throw;
            }
        }
        void PumpOutput(string path) {
            try {
                using (StreamWriter log = new StreamWriter(path, false, new UTF8Encoding(false))) {
                    StringBuilder line = new StringBuilder();
                    long total = 0;
                    for (;;) {
                        int next = process.StandardOutput.Read();
                        if (next < 0) {
                            if (line.Length != 0) { log.Write(line.ToString()); Fail("EOF in an MCP frame"); }
                            break;
                        }
                        if (++total > 64 * 1024 * 1024 || line.Length >= 8 * 1024 * 1024)
                            throw new IOException("MCP output exceeds smoke evidence budget");
                        if (next == '\n') {
                            string value = line.ToString().TrimEnd('\r'); line.Length = 0;
                            log.WriteLine(value); log.Flush();
                            if (!messages.TryAdd(value, 1000)) throw new IOException("MCP response queue is full");
                        } else line.Append((char)next);
                    }
                }
            } catch (Exception e) { Fail("stdout: " + e.Message); }
            finally { messages.CompleteAdding(); }
        }
        void PumpError(string path) {
            try {
                using (StreamWriter log = new StreamWriter(path, false, new UTF8Encoding(false))) {
                    char[] buffer = new char[1024]; int count; long total = 0;
                    while ((count = process.StandardError.Read(buffer, 0, buffer.Length)) > 0) {
                        total += count;
                        if (total > 2 * 1024 * 1024) throw new IOException("stderr exceeds smoke evidence budget");
                        log.Write(buffer, 0, count); log.Flush();
                    }
                }
            } catch (Exception e) { Fail("stderr: " + e.Message); }
        }
        public void Send(string frame) {
            byte[] bytes = new UTF8Encoding(false).GetBytes(frame + "\n");
            if (bytes.Length > 65536) throw new IOException("Request exceeds smoke frame budget");
            Task send = Task.Factory.StartNew(delegate {
                Stream stream = process.StandardInput.BaseStream;
                stream.Write(bytes, 0, bytes.Length); stream.Flush();
            });
            if (!send.Wait(5000)) throw new TimeoutException("MCP stdin write timed out; never replay this request");
        }
        public string Receive(int milliseconds) {
            Stopwatch clock = Stopwatch.StartNew();
            while (clock.ElapsedMilliseconds < milliseconds) {
                if (failure != null) throw new IOException(failure);
                string frame;
                if (messages.TryTake(out frame, Math.Min(50, milliseconds))) return frame;
                if (messages.IsCompleted) throw new EndOfStreamException("MCP stdout closed");
            }
            throw new TimeoutException("MCP response timed out; effects are unknown and must not be replayed");
        }
        public object Close() {
            bool forced = false;
            string cleanupError = null;
            try {
                Task close = Task.Factory.StartNew(delegate { process.StandardInput.Close(); });
                if (!close.Wait(250)) cleanupError = "stdin close did not finish within 250 ms";
                if (!process.WaitForExit(3000)) {
                    forced = true; process.Kill();
                    if (!process.WaitForExit(1000)) cleanupError = "MCP process did not exit after kill";
                }
            } catch (Exception e) {
                cleanupError = e.Message;
                try { if (!process.HasExited) { forced = true; process.Kill(); process.WaitForExit(1000); } } catch { }
            }
            bool readers = stdout.Join(250) & stderr.Join(250);
            bool exited = process.HasExited;
            return new { pid = process.Id, exited = exited, forced = forced,
                exit_code = exited ? (int?)process.ExitCode : null,
                readers_joined = readers, transport_error = failure, cleanup_error = cleanupError };
        }
    }
}

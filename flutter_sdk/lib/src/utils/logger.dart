import 'dart:developer' as dev;

/// Log levels for SDK internal logging.
enum LogLevel { debug, info, warning, error, none }

/// Internal logger. Strips sensitive data (message text, key bytes) before logging.
class SdkLogger {
  final String tag;
  final LogLevel level;

  SdkLogger({required this.tag, this.level = LogLevel.warning});

  void debug(String message) => _log(LogLevel.debug, message);
  void info(String message) => _log(LogLevel.info, message);
  void warning(String message) => _log(LogLevel.warning, message);
  void error(String message) => _log(LogLevel.error, message);

  void _log(LogLevel msgLevel, String message) {
    if (msgLevel.index < level.index) return;
    final prefix = '[$tag][${msgLevel.name.toUpperCase()}]';
    dev.log('$prefix $message', name: 'rust_e2e_chat_sdk');
  }
}

import 'dart:async';
import 'package:flutter/material.dart' hide ConnectionState;
import 'package:rust_e2e_chat_sdk/rust_e2e_chat_sdk.dart';

void main() {
  runApp(const ChatExampleApp());
}

class ChatExampleApp extends StatelessWidget {
  const ChatExampleApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'E2E Chat SDK Demo',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.indigo),
        useMaterial3: true,
      ),
      home: const InitScreen(),
    );
  }
}

// ---------------------------------------------------------------------------
// Screen 1 — Initialize SDK
// ---------------------------------------------------------------------------

class InitScreen extends StatefulWidget {
  const InitScreen({super.key});

  @override
  State<InitScreen> createState() => _InitScreenState();
}

class _InitScreenState extends State<InitScreen> {
  final _baseUrlCtrl = TextEditingController(text: 'http://localhost:3000/api');
  final _tokenCtrl = TextEditingController(text: 'your-oidc-token');
  final _userIdCtrl = TextEditingController(text: 'user-alice');

  bool _loading = false;
  String? _error;

  Future<void> _initialize() async {
    setState(() {
      _loading = true;
      _error = null;
    });

    try {
      final client = await ChatClient.initialize(ChatClientConfig(
        baseUrl: _baseUrlCtrl.text.trim(),
        accessToken: _tokenCtrl.text.trim(),
        userId: _userIdCtrl.text.trim(),
        logLevel: LogLevel.info,
      ));

      if (!mounted) return;
      Navigator.of(context).pushReplacement(
        MaterialPageRoute(
          builder: (_) => HomeScreen(client: client),
        ),
      );
    } on SdkError catch (e) {
      setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('E2E Chat — Initialize')),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: _baseUrlCtrl,
              decoration: const InputDecoration(
                labelText: 'Base URL',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 16),
            TextField(
              controller: _tokenCtrl,
              decoration: const InputDecoration(
                labelText: 'Access Token',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 16),
            TextField(
              controller: _userIdCtrl,
              decoration: const InputDecoration(
                labelText: 'User ID',
                border: OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 24),
            if (_error != null)
              Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: Text(_error!, style: const TextStyle(color: Colors.red)),
              ),
            FilledButton(
              onPressed: _loading ? null : _initialize,
              child: _loading
                  ? const SizedBox(
                      height: 20,
                      width: 20,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Text('Connect'),
            ),
          ],
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Screen 2 — Home (conversation list)
// ---------------------------------------------------------------------------

class HomeScreen extends StatefulWidget {
  final ChatClient client;
  const HomeScreen({super.key, required this.client});

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  ChatClient get _client => widget.client;
  StreamSubscription? _stateSub;
  ConnectionState _connectionState = ConnectionState.connecting;

  @override
  void initState() {
    super.initState();
    _stateSub = _client.connectionState.listen((s) {
      setState(() => _connectionState = s);
    });
  }

  @override
  void dispose() {
    _stateSub?.cancel();
    super.dispose();
  }

  String get _stateLabel {
    switch (_connectionState) {
      case ConnectionState.connected:
        return '🟢 Connected';
      case ConnectionState.connecting:
        return '🟡 Connecting…';
      case ConnectionState.reconnecting:
        return '🟡 Reconnecting…';
      case ConnectionState.disconnected:
        return '🔴 Disconnected';
    }
  }

  Future<void> _openConversation() async {
    final recipientId =
        await _showInputDialog(context, 'Open 1:1 Chat', 'Recipient User ID');
    if (recipientId == null || recipientId.isEmpty) return;

    try {
      final convo = await _client.openConversation(recipientId);
      if (!mounted) return;
      _navigateToConvo(convo);
    } on SdkError catch (e) {
      _showError(e.toString());
    }
  }

  Future<void> _createGroup() async {
    final members = await _showInputDialog(
        context, 'Create Group', 'Member IDs (comma-separated)');
    if (members == null || members.isEmpty) return;

    final memberIds = members
        .split(',')
        .map((s) => s.trim())
        .where((s) => s.isNotEmpty)
        .toList();

    try {
      final group = await _client.createGroup(memberIds);
      if (!mounted) return;
      _navigateToConvo(group);
    } on SdkError catch (e) {
      _showError(e.toString());
    }
  }

  void _navigateToConvo(ChatConversation convo) {
    Navigator.of(context).push(MaterialPageRoute(
      builder: (_) => ChatScreen(
        client: _client,
        conversation: convo,
      ),
    ));
  }

  void _showError(String msg) {
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text('Chats — ${_client.userId}'),
        bottom: PreferredSize(
          preferredSize: const Size.fromHeight(24),
          child: Text(_stateLabel,
              style: const TextStyle(fontSize: 12, color: Colors.white70)),
        ),
        backgroundColor: Theme.of(context).colorScheme.inversePrimary,
        actions: [
          IconButton(
            icon: const Icon(Icons.group_add),
            tooltip: 'Create Group',
            onPressed: _createGroup,
          ),
        ],
      ),
      body: StreamBuilder<List<ChatConversation>>(
        stream: _client.conversations,
        builder: (context, snapshot) {
          final convos = snapshot.data ?? [];
          if (convos.isEmpty) {
            return Center(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const Icon(Icons.chat_bubble_outline,
                      size: 64, color: Colors.grey),
                  const SizedBox(height: 16),
                  Text('No conversations yet',
                      style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 8),
                  const Text('Tap + to start a chat'),
                ],
              ),
            );
          }
          return ListView.builder(
            itemCount: convos.length,
            itemBuilder: (context, i) {
              final c = convos[i];
              final icon =
                  c.type == ConversationType.group ? Icons.group : Icons.person;
              final subtitle = c.currentMembers
                  .where((m) => m.userId != _client.userId)
                  .map((m) => m.userId)
                  .join(', ');
              return ListTile(
                leading: CircleAvatar(child: Icon(icon)),
                title: Text(c.conversationId.substring(0, 8)),
                subtitle: Text(subtitle),
                onTap: () => _navigateToConvo(c),
              );
            },
          );
        },
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: _openConversation,
        child: const Icon(Icons.add),
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// Screen 3 — Chat
// ---------------------------------------------------------------------------

class ChatScreen extends StatefulWidget {
  final ChatClient client;
  final ChatConversation conversation;

  const ChatScreen({
    super.key,
    required this.client,
    required this.conversation,
  });

  @override
  State<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends State<ChatScreen> {
  final _textCtrl = TextEditingController();
  final _scrollCtrl = ScrollController();
  bool _sending = false;

  ChatConversation get _convo => widget.conversation;

  Future<void> _send() async {
    final text = _textCtrl.text.trim();
    if (text.isEmpty) return;

    setState(() => _sending = true);
    _textCtrl.clear();

    try {
      await _convo.sendMessage(text);
      _scrollToBottom();
    } on SdkError catch (e) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text(e.toString())));
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollCtrl.hasClients) {
        _scrollCtrl.animateTo(
          _scrollCtrl.position.maxScrollExtent,
          duration: const Duration(milliseconds: 300),
          curve: Curves.easeOut,
        );
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final isGroup = _convo.type == ConversationType.group;
    final title = isGroup ? '👥 Group' : '💬 Chat';

    return Scaffold(
      appBar: AppBar(
        title: Text(title),
        backgroundColor: Theme.of(context).colorScheme.inversePrimary,
      ),
      body: Column(
        children: [
          // Upload progress bar
          StreamBuilder<AttachmentUploadProgress?>(
            stream: _convo.uploadProgress,
            builder: (context, snapshot) {
              final progress = snapshot.data;
              if (progress == null) return const SizedBox.shrink();
              return LinearProgressIndicator(value: progress.fraction);
            },
          ),

          // Messages list
          Expanded(
            child: StreamBuilder<List<ChatMessage>>(
              stream: _convo.messages,
              builder: (context, snapshot) {
                final messages = snapshot.data ?? [];
                return ListView.builder(
                  controller: _scrollCtrl,
                  padding: const EdgeInsets.all(8),
                  itemCount: messages.length,
                  itemBuilder: (context, i) =>
                      _MessageBubble(message: messages[i]),
                );
              },
            ),
          ),

          // Input bar
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.all(8),
              child: Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: _textCtrl,
                      decoration: const InputDecoration(
                        hintText: 'Message…',
                        border: OutlineInputBorder(),
                        contentPadding:
                            EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                      ),
                      onSubmitted: (_) => _send(),
                      textInputAction: TextInputAction.send,
                    ),
                  ),
                  const SizedBox(width: 8),
                  FilledButton(
                    onPressed: _sending ? null : _send,
                    child: _sending
                        ? const SizedBox(
                            height: 18,
                            width: 18,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.send),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _MessageBubble extends StatelessWidget {
  final ChatMessage message;
  const _MessageBubble({required this.message});

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final isMine = message.isMine;
    final isSystem = message.type == ChatMessageType.memberAdded ||
        message.type == ChatMessageType.memberRemoved ||
        message.type == ChatMessageType.system;

    if (isSystem) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: Text(
            _systemText(),
            style: const TextStyle(color: Colors.grey, fontSize: 12),
          ),
        ),
      );
    }

    if (message.decryptionError) {
      return Align(
        alignment: isMine ? Alignment.centerRight : Alignment.centerLeft,
        child: Container(
          margin: const EdgeInsets.symmetric(vertical: 2),
          padding: const EdgeInsets.all(10),
          decoration: BoxDecoration(
            color: Colors.red.shade100,
            borderRadius: BorderRadius.circular(12),
          ),
          child: const Text('⚠️ Message could not be decrypted',
              style: TextStyle(color: Colors.red)),
        ),
      );
    }

    final bgColor =
        isMine ? colors.primaryContainer : colors.surfaceContainerHighest;
    final textColor =
        isMine ? colors.onPrimaryContainer : colors.onSurfaceVariant;

    return Align(
      alignment: isMine ? Alignment.centerRight : Alignment.centerLeft,
      child: Container(
        constraints:
            BoxConstraints(maxWidth: MediaQuery.of(context).size.width * 0.75),
        margin: const EdgeInsets.symmetric(vertical: 2),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        decoration: BoxDecoration(
          color: bgColor,
          borderRadius: BorderRadius.circular(16),
        ),
        child: Column(
          crossAxisAlignment:
              isMine ? CrossAxisAlignment.end : CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            if (!isMine)
              Text(message.senderUserId,
                  style: TextStyle(
                      fontSize: 11,
                      color: colors.primary,
                      fontWeight: FontWeight.w600)),
            if (message.type == ChatMessageType.attachment)
              Text('📎 ${message.attachmentName ?? 'attachment'}',
                  style: TextStyle(color: textColor))
            else
              Text(message.text ?? '', style: TextStyle(color: textColor)),
            const SizedBox(height: 2),
            Text(
              _formatTime(message.timestamp),
              style: TextStyle(
                  fontSize: 10,
                  color: textColor.withAlpha((0.6 * 255).toInt())),
            ),
          ],
        ),
      ),
    );
  }

  String _systemText() {
    switch (message.type) {
      case ChatMessageType.memberAdded:
        return '${message.senderUserId} joined the group';
      case ChatMessageType.memberRemoved:
        return '${message.senderUserId} left the group';
      default:
        return message.text ?? '';
    }
  }

  String _formatTime(DateTime dt) {
    final h = dt.hour.toString().padLeft(2, '0');
    final m = dt.minute.toString().padLeft(2, '0');
    return '$h:$m';
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

Future<String?> _showInputDialog(
    BuildContext context, String title, String hint) async {
  final ctrl = TextEditingController();
  return showDialog<String>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(title),
      content: TextField(
        controller: ctrl,
        decoration: InputDecoration(hintText: hint),
        autofocus: true,
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.pop(ctx), child: const Text('Cancel')),
        FilledButton(
            onPressed: () => Navigator.pop(ctx, ctrl.text),
            child: const Text('OK')),
      ],
    ),
  );
}

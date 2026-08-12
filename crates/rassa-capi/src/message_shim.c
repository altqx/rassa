#include <stdarg.h>
#include <stddef.h>

#if defined(_WIN32)
#define RASSA_HIDDEN
#else
#define RASSA_HIDDEN __attribute__((visibility("hidden")))
#endif

typedef void (*RassaMessageCallback)(int level, const char *format,
                                     va_list arguments, void *data);
typedef void (*RassaFormattedMessageSink)(int level, const char *message,
                                          void *data);

typedef struct RassaFormattedMessageBridge {
    RassaFormattedMessageSink sink;
    void *data;
} RassaFormattedMessageBridge;

typedef union RassaCallbackPointer {
    void *opaque;
    RassaMessageCallback callback;
} RassaCallbackPointer;

static RassaMessageCallback callback_from_opaque(void *opaque)
{
    RassaCallbackPointer pointer;
    pointer.opaque = opaque;
    return pointer.callback;
}

static void *callback_to_opaque(RassaMessageCallback callback)
{
    RassaCallbackPointer pointer;
    pointer.callback = callback;
    return pointer.opaque;
}

static void invoke_variadic(RassaMessageCallback callback, int level,
                            void *data, const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    callback(level, format, arguments, data);
    va_end(arguments);
}

RASSA_HIDDEN void rassa_emit_message(void *opaque_callback, int level,
                                     const char *message, void *data)
{
    RassaMessageCallback callback = callback_from_opaque(opaque_callback);
    if (callback && message)
        invoke_variadic(callback, level, data, "%s", message);
}

static void formatted_sink_callback(int level, const char *format,
                                    va_list arguments, void *data)
{
    RassaFormattedMessageBridge *bridge = data;
    const char *message;
    if (!bridge || !bridge->sink)
        return;
    if (!format || format[0] != '%' || format[1] != 's' || format[2] != '\0')
        return;
    message = va_arg(arguments, const char *);
    bridge->sink(level, message, bridge->data);
}

RASSA_HIDDEN void *rassa_formatted_sink_callback_pointer(void)
{
    return callback_to_opaque(formatted_sink_callback);
}

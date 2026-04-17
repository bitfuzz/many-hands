#ifndef apple_intelligence_bridge_h
#define apple_intelligence_bridge_h

// C-compatible function declarations for Swift bridge

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    char* response;
    int success; // 0 for failure, 1 for success
    char* error_message; // Only valid when success = 0
} AppleLLMResponse;

typedef struct {
    float* samples;
    unsigned long long sample_count;
    int success; // 0 for failure, 1 for success
    char* error_message; // Only valid when success = 0
} SystemAudioCaptureResponse;

// Check if Apple Intelligence is available on the device
int is_apple_intelligence_available(void);

// Process text using Apple's on-device LLM with separate system prompt and user content
AppleLLMResponse* process_text_with_system_prompt_apple(const char* system_prompt, const char* user_content, int max_tokens);

// Free memory allocated by the Apple LLM response
void free_apple_llm_response(AppleLLMResponse* response);

// Screen capture permission helpers
int preflight_screen_capture_access(void);
int request_screen_capture_access(void);

// System audio capture helpers
int start_system_audio_capture(void);
SystemAudioCaptureResponse* stop_system_audio_capture(void);
int get_system_audio_levels(float* out_levels, unsigned long long capacity);
void free_system_audio_capture_response(SystemAudioCaptureResponse* response);

#ifdef __cplusplus
}
#endif

#endif /* apple_intelligence_bridge_h */
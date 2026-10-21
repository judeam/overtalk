import React, { useState, useEffect } from 'react';
import { Globe } from 'lucide-react';
import { toast } from 'sonner';
import { useConfig } from '@/contexts/ConfigContext';
import { invoke } from '@tauri-apps/api/core';

interface Language {
  code: string;
  name: string;
}

// ISO 639-1 language codes supported by Whisper
const LANGUAGES: Language[] = [
  { code: 'auto', name: 'Auto Detect (Original Language)' },
  { code: 'auto-translate', name: 'Auto Detect (Translate to English)' },
  { code: 'en', name: 'English' },
  { code: 'zh', name: 'Chinese' },
  { code: 'de', name: 'German' },
  { code: 'es', name: 'Spanish' },
  { code: 'ru', name: 'Russian' },
  { code: 'ko', name: 'Korean' },
  { code: 'fr', name: 'French' },
  { code: 'ja', name: 'Japanese' },
  { code: 'pt', name: 'Portuguese' },
  { code: 'tr', name: 'Turkish' },
  { code: 'pl', name: 'Polish' },
  { code: 'ca', name: 'Catalan' },
  { code: 'nl', name: 'Dutch' },
  { code: 'ar', name: 'Arabic' },
  { code: 'sv', name: 'Swedish' },
  { code: 'it', name: 'Italian' },
  { code: 'id', name: 'Indonesian' },
  { code: 'hi', name: 'Hindi' },
  { code: 'fi', name: 'Finnish' },
  { code: 'vi', name: 'Vietnamese' },
  { code: 'he', name: 'Hebrew' },
  { code: 'uk', name: 'Ukrainian' },
  { code: 'el', name: 'Greek' },
  { code: 'ms', name: 'Malay' },
  { code: 'cs', name: 'Czech' },
  { code: 'ro', name: 'Romanian' },
  { code: 'da', name: 'Danish' },
  { code: 'hu', name: 'Hungarian' },
  { code: 'ta', name: 'Tamil' },
  { code: 'no', name: 'Norwegian' },
  { code: 'th', name: 'Thai' },
  { code: 'ur', name: 'Urdu' },
  { code: 'hr', name: 'Croatian' },
  { code: 'bg', name: 'Bulgarian' },
  { code: 'lt', name: 'Lithuanian' },
  { code: 'la', name: 'Latin' },
  { code: 'mi', name: 'Maori' },
  { code: 'ml', name: 'Malayalam' },
  { code: 'cy', name: 'Welsh' },
  { code: 'sk', name: 'Slovak' },
  { code: 'te', name: 'Telugu' },
  { code: 'fa', name: 'Persian' },
  { code: 'lv', name: 'Latvian' },
  { code: 'bn', name: 'Bengali' },
  { code: 'sr', name: 'Serbian' },
  { code: 'az', name: 'Azerbaijani' },
  { code: 'sl', name: 'Slovenian' },
  { code: 'kn', name: 'Kannada' },
  { code: 'et', name: 'Estonian' },
  { code: 'mk', name: 'Macedonian' },
  { code: 'br', name: 'Breton' },
  { code: 'eu', name: 'Basque' },
  { code: 'is', name: 'Icelandic' },
  { code: 'hy', name: 'Armenian' },
  { code: 'ne', name: 'Nepali' },
  { code: 'mn', name: 'Mongolian' },
  { code: 'bs', name: 'Bosnian' },
  { code: 'kk', name: 'Kazakh' },
  { code: 'sq', name: 'Albanian' },
  { code: 'sw', name: 'Swahili' },
  { code: 'gl', name: 'Galician' },
  { code: 'mr', name: 'Marathi' },
  { code: 'pa', name: 'Punjabi' },
  { code: 'si', name: 'Sinhala' },
  { code: 'km', name: 'Khmer' },
  { code: 'sn', name: 'Shona' },
  { code: 'yo', name: 'Yoruba' },
  { code: 'so', name: 'Somali' },
  { code: 'af', name: 'Afrikaans' },
  { code: 'oc', name: 'Occitan' },
  { code: 'ka', name: 'Georgian' },
  { code: 'be', name: 'Belarusian' },
  { code: 'tg', name: 'Tajik' },
  { code: 'sd', name: 'Sindhi' },
  { code: 'gu', name: 'Gujarati' },
  { code: 'am', name: 'Amharic' },
  { code: 'yi', name: 'Yiddish' },
  { code: 'lo', name: 'Lao' },
  { code: 'uz', name: 'Uzbek' },
  { code: 'fo', name: 'Faroese' },
  { code: 'ht', name: 'Haitian Creole' },
  { code: 'ps', name: 'Pashto' },
  { code: 'tk', name: 'Turkmen' },
  { code: 'nn', name: 'Norwegian Nynorsk' },
  { code: 'mt', name: 'Maltese' },
  { code: 'sa', name: 'Sanskrit' },
  { code: 'lb', name: 'Luxembourgish' },
  { code: 'my', name: 'Myanmar' },
  { code: 'bo', name: 'Tibetan' },
  { code: 'tl', name: 'Tagalog' },
  { code: 'mg', name: 'Malagasy' },
  { code: 'as', name: 'Assamese' },
  { code: 'tt', name: 'Tatar' },
  { code: 'haw', name: 'Hawaiian' },
  { code: 'ln', name: 'Lingala' },
  { code: 'ha', name: 'Hausa' },
  { code: 'ba', name: 'Bashkir' },
  { code: 'jw', name: 'Javanese' },
  { code: 'su', name: 'Sundanese' },
];

const QWEN_LANGUAGE_CODES = new Set([
  'auto', 'zh', 'en', 'yue', 'ar', 'de', 'fr', 'es', 'pt', 'id', 'it', 'ko',
  'ru', 'th', 'vi', 'ja', 'tr', 'hi', 'ms', 'nl', 'sv', 'da', 'fi', 'pl',
  'cs', 'fil', 'fa', 'el', 'hu', 'mk', 'ro',
]);
const NEMOTRON_LANGUAGE_CODES = new Set([
  'auto', 'en', 'es', 'fr', 'it', 'pt', 'nl', 'de', 'tr', 'ru', 'ar', 'hi',
  'ja', 'ko', 'vi', 'uk', 'pl', 'sv', 'cs', 'no', 'da', 'bg', 'fi', 'hr',
  'sk', 'zh', 'hu', 'ro', 'et',
]);
const QWEN_LANGUAGES = [
  ...LANGUAGES.filter(language => QWEN_LANGUAGE_CODES.has(language.code)),
  { code: 'yue', name: 'Cantonese' }, { code: 'fil', name: 'Filipino' },
];
const NEMOTRON_LANGUAGES = [
  ...LANGUAGES.filter(language => NEMOTRON_LANGUAGE_CODES.has(language.code)),
  { code: 'en-GB', name: 'English (United Kingdom)' },
  { code: 'es-US', name: 'Spanish (United States)' },
  { code: 'fr-CA', name: 'French (Canada)' },
  { code: 'pt-PT', name: 'Portuguese (Portugal)' },
];
interface SpeechLanguageSettings {
  live_diarization: boolean;
  streaming_asr_model: string;
}

interface LanguageSelectionProps {
  selectedLanguage: string;
  onLanguageChange: (language: string) => void;
  disabled?: boolean;
  provider?: 'localWhisper' | 'parakeet' | 'localSpeech' | 'deepgram' | 'elevenLabs' | 'groq' | 'openai';
}

export function LanguageSelection({
  selectedLanguage,
  onLanguageChange,
  disabled = false,
  provider = 'localWhisper'
}: LanguageSelectionProps) {
  const [saving, setSaving] = useState(false);
  const [speechSettings, setSpeechSettings] = useState<SpeechLanguageSettings | null>(null);
  const [capabilityError, setCapabilityError] = useState<string | null>(null);
  const { setSelectedLanguage, transcriptModelConfig } = useConfig();

  useEffect(() => {
    if (provider !== 'localSpeech') return;
    let active = true;
    const refresh = () => {
      setCapabilityError(null);
      invoke<SpeechLanguageSettings>('speech_get_settings')
        .then(settings => { if (active) setSpeechSettings(settings); })
        .catch(error => { if (active) setCapabilityError(String(error)); });
    };
    refresh();
    window.addEventListener('sonefold-transcription-model-changed', refresh);
    return () => {
      active = false;
      window.removeEventListener('sonefold-transcription-model-changed', refresh);
    };
  }, [provider]);

  const isLocalSpeech = provider === 'localSpeech';
  const isLiveSpeech = isLocalSpeech && speechSettings?.live_diarization === true;
  const isEnglishStreaming = isLiveSpeech && speechSettings?.streaming_asr_model === 'multitalker-parakeet';
  const isParakeet = provider === 'parakeet' || (isLocalSpeech && !isLiveSpeech && transcriptModelConfig.model === 'parakeet-tdt-0.6b-v3');
  const capabilitiesReady = !isLocalSpeech || speechSettings !== null;
  const availableLanguages = !capabilitiesReady || isParakeet
    ? LANGUAGES.filter(language => language.code === 'auto')
    : isEnglishStreaming
      ? LANGUAGES.filter(language => language.code === 'auto' || language.code === 'en')
      : isLiveSpeech ? NEMOTRON_LANGUAGES : isLocalSpeech ? QWEN_LANGUAGES : LANGUAGES;
  const selectedSupported = availableLanguages.some(language => language.code === selectedLanguage);
  const effectiveLanguage = selectedSupported ? selectedLanguage : 'auto';

  useEffect(() => {
    if (!disabled && capabilitiesReady && !selectedSupported) {
      setSelectedLanguage('auto');
      onLanguageChange('auto');
      toast.info('Language changed to automatic detection', {
        description: 'The selected speech model does not support the previous language setting.',
      });
    }
  }, [disabled, capabilitiesReady, selectedSupported, onLanguageChange, setSelectedLanguage]);

  const handleLanguageChange = async (languageCode: string) => {
    setSaving(true);
    try {
      // Save language preference to localStorage and sync to backend
      setSelectedLanguage(languageCode);
      onLanguageChange(languageCode);
      console.log('Language preference saved:', languageCode);

      // Show success toast
      const selectedLang = availableLanguages.find(lang => lang.code === languageCode);
      const languageName = selectedLang?.name || languageCode;
      toast.success("Language preference saved", {
        description: `Transcription language set to ${languageName}`
      });
    } catch (error) {
      console.error('Failed to save language preference:', error);
      toast.error("Failed to save language preference", {
        description: error instanceof Error ? error.message : String(error)
      });
    } finally {
      setSaving(false);
    }
  };

  // Find the selected language name for display
  const selectedLanguageName = availableLanguages.find(
    lang => lang.code === effectiveLanguage
  )?.name || 'Auto Detect (Original Language)';

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Globe className="h-4 w-4 text-gray-600" />
          <h4 className="text-sm font-medium text-gray-900">Transcription Language</h4>
        </div>
      </div>

      <div className="space-y-2">
        <select
          value={effectiveLanguage}
          onChange={(e) => handleLanguageChange(e.target.value)}
          disabled={disabled || saving || !capabilitiesReady}
          className="w-full px-3 py-2 text-sm bg-card border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 disabled:bg-gray-50 disabled:text-gray-500"
        >
          {availableLanguages.map((language) => (
            <option key={language.code} value={language.code}>
              {language.name}
              {language.code !== 'auto' && language.code !== 'auto-translate' && ` (${language.code})`}
            </option>
          ))}
        </select>

        {/* Parakeet language limitation warning */}
        {isParakeet && (
          <div className="p-2 bg-amber-50 border border-amber-200 rounded text-amber-800">
            <p className="font-medium">Parakeet language support</p>
            <p className="mt-1 text-xs">V3 automatically detects its 25 supported European languages. V2 transcribes English. Parakeet produces the original spoken language.</p>
          </div>
        )}
        {isLocalSpeech && !isParakeet && (
          <p className="text-xs text-gray-600">
            {isEnglishStreaming ? 'Multitalker Parakeet transcribes English with live speaker recognition.'
              : isLiveSpeech ? 'Nemotron 3.5 supports 32 locales for live transcription; accuracy varies by language.'
                : 'Qwen3-ASR supports 30 languages and transcribes the original spoken language.'}
          </p>
        )}
        {isLocalSpeech && capabilityError && <p role="alert" className="text-xs text-red-700">Could not load speech model capabilities: {capabilityError}</p>}

        {/* Info text */}
        <div className="text-xs space-y-2 pt-2">
          <p className="text-gray-600">
            <strong>Current:</strong> {selectedLanguageName}
          </p>
          {effectiveLanguage === 'auto' && !isParakeet && !isEnglishStreaming && capabilitiesReady && (
            <div className="p-2 bg-yellow-50 border border-yellow-200 rounded text-yellow-800">
              <p className="font-medium">⚠️ Auto Detect may produce incorrect results</p>
              <p className="mt-1">For best accuracy, select your specific language (e.g., English, Spanish, etc.)</p>
            </div>
          )}
          {effectiveLanguage === 'auto-translate' && (
            <div className="p-2 bg-blue-50 border border-blue-200 rounded text-blue-800">
              <p className="font-medium">🌐 Translation Mode Active</p>
              <p className="mt-1">All audio will be automatically translated to English. Best for multilingual meetings where you need English output.</p>
            </div>
          )}
          {effectiveLanguage !== 'auto' && effectiveLanguage !== 'auto-translate' && (
            <p className="text-gray-600">
              Transcription will be optimized for <strong>{selectedLanguageName}</strong>
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

import { useState, useEffect, useCallback } from 'react';
import { invoke as invokeTauri } from '@tauri-apps/api/core';
import { toast } from 'sonner';

export function useTemplates() {
  const [availableTemplates, setAvailableTemplates] = useState<Array<{
    id: string;
    name: string;
    description: string;
  }>>([]);
  const [selectedTemplate, setSelectedTemplate] = useState<string>('standard_meeting');
  const refreshTemplates = useCallback(async () => {
    const templates = await invokeTauri<Array<{ id: string; name: string; description: string }>>('api_list_templates');
    setAvailableTemplates(templates);
    setSelectedTemplate(previous => {
      let stored = previous;
      try { stored = window.localStorage.getItem('sonefold-summary-template') || previous; }
      catch { /* Selection still works when WebView storage is unavailable. */ }
      return templates.some(template => template.id === stored) ? stored : 'standard_meeting';
    });
  }, []);

  // Fetch available templates on mount
  useEffect(() => {
    const reload = () => { void refreshTemplates().catch(error => console.error('Failed to fetch templates:', error)); };
    reload();
    window.addEventListener('sonefold-templates-changed', reload);
    return () => window.removeEventListener('sonefold-templates-changed', reload);
  }, [refreshTemplates]);

  // Handle template selection
  const handleTemplateSelection = useCallback((templateId: string, templateName: string) => {
    setSelectedTemplate(templateId);
    try { window.localStorage.setItem('sonefold-summary-template', templateId); }
    catch { /* Preserve the current session's selection. */ }
    toast.success('Template selected', {
      description: `Using "${templateName}" template for summary generation`,
    });
  }, []);

  return {
    availableTemplates,
    selectedTemplate,
    handleTemplateSelection,
    refreshTemplates,
  };
}

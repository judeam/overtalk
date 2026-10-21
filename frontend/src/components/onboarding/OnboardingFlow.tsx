import React, { useEffect } from 'react';
import { useOnboarding } from '@/contexts/OnboardingContext';
import {
  WelcomeStep,
  PermissionsStep,
  DownloadProgressStep,
} from './steps';
import { AudioCheckStep } from './steps/AudioCheckStep';

export function OnboardingFlow() {
  const { currentStep } = useOnboarding();
  const [isMac, setIsMac] = React.useState(false);

  useEffect(() => {
    // Check if running on macOS
    const checkPlatform = async () => {
      try {
        // Dynamic import to avoid SSR issues if any
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch (e) {
        console.error('Failed to detect platform:', e);
        // Fallback
        setIsMac(navigator.userAgent.includes('Mac'));
      }
    };
    checkPlatform();
  }, []);

  // Welcome, speech model, (macOS) permissions, then a sound check; the tour follows on first launch.
  const total = isMac ? 4 : 3;
  return (
    <div className="onboarding-flow">
      {currentStep === 1 && <WelcomeStep />}
      {currentStep === 2 && <DownloadProgressStep />}
      {currentStep === 3 && (isMac ? <PermissionsStep /> : <AudioCheckStep step={3} totalSteps={total} />)}
      {currentStep === 4 && <AudioCheckStep step={4} totalSteps={total} />}
    </div>
  );
}

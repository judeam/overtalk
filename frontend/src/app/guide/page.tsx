'use client';

import { Suspense } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { useGuide } from '@/components/guide/GuideProvider';
import { Guide } from '@/components/guide/Guide';

function GuidePage() {
  const router = useRouter();
  const { startTour } = useGuide();
  const chapter = useSearchParams().get('chapter');
  return <Guide initialChapter={chapter} startTour={startTour} standalone onClose={() => router.back()} />;
}

export default function Page() {
  return (
    <Suspense fallback={null}>
      <GuidePage />
    </Suspense>
  );
}

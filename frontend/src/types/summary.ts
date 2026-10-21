interface Summary {
    key_points: string[];
    action_items: string[];
    decisions: string[];
    main_topics: string[];
    participants?: string[];
}

export interface SummaryResponse {
    summary: Summary;
    raw_summary?: string;
}

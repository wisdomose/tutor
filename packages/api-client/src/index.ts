// Typed client for the Tutor API. Regenerate the types after changing the API:
//   pnpm --filter @tutor/api-client generate
import createClient from 'openapi-fetch'
import type { components, paths } from './schema'

export type Schemas = components['schemas']
export type Subject = Schemas['Subject']
export type Topic = Schemas['Topic']
export type Exam = Schemas['Exam']
export type Source = Schemas['Source']
export type NewSource = Schemas['NewSource']
export type QueueStatus = Schemas['QueueStatus']
export type UsageSummary = Schemas['UsageSummary']
export type PageSummary = Schemas['PageSummary']
export type PageDetail = Schemas['PageDetail']
export type QuestionSummary = Schemas['QuestionSummary']
export type QuestionList = Schemas['QuestionList']
export type QuestionDetail = Schemas['QuestionDetail']
export type QuestionEdit = Schemas['QuestionEdit']
export type Diagram = Schemas['Diagram']
export type OptionDetail = Schemas['OptionDetail']
export type Health = Schemas['Health']
export type QuestionGroup = Schemas['QuestionGroup']

export type CheckReason =
  | 'conflicting_answers'
  | 'has_diagram'
  | 'low_confidence'
  | 'no_answer'
  | 'refers_to_number'

export const checkReasonLabels: Record<CheckReason, string> = {
  conflicting_answers: 'Conflicting answers',
  has_diagram: 'Has diagram',
  low_confidence: 'Low confidence',
  no_answer: 'No answer',
  refers_to_number: 'Refers to a question number',
}

export function createApiClient(baseUrl: string) {
  return createClient<paths>({ baseUrl })
}

export type ApiClient = ReturnType<typeof createApiClient>

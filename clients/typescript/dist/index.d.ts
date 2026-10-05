import { SessionTransport, DesktopError, type Reply } from '../runtime/transport.mjs';
import { PlanBuilder } from '../runtime/plan.mjs';
import type { ActOptions, GrantStatus, Observation, ObserveArgs, ReadResult, Receipt, Ref, SessionOptions, Step } from './types.js';
export * from './types.js';
export { PlanBuilder, DesktopError };
export type { Reply };
export declare class DesktopClient {
    private transport;
    private queries;
    private building;
    constructor(transport: SessionTransport);
    call<T = unknown>(op: string, args?: unknown, id?: string): Promise<T>;
    observe(args?: ObserveArgs | Ref): Promise<Observation>;
    outline(ref: Ref, options?: ObserveArgs): Promise<Observation>;
    find(within: Ref, locator: Omit<NonNullable<ObserveArgs['match']>, 'within'>, options?: ObserveArgs): Promise<Observation>;
    next(ob: Observation): Promise<Observation>;
    plan(): PlanBuilder;
    act(steps: Step[] | PlanBuilder, options?: ActOptions, id?: string): Promise<Receipt>;
    transaction(build: (tx: PlanBuilder) => void | Promise<void>, options?: ActOptions, id?: string): Promise<Receipt>;
    read(ref: Ref, options?: Record<string, unknown>): Promise<ReadResult>;
    sync(cursor: string, options?: Record<string, unknown>): Promise<unknown>;
    capture(args: Record<string, unknown>): Promise<unknown>;
    get(runId: string): Promise<Receipt>;
    cancel(runId: string): Promise<Receipt>;
    reconcile(id: string): Promise<Reply<Receipt>>;
    set(ref: Ref, text: string, id?: string): Promise<Receipt>;
    invoke(ref: Ref, id?: string): Promise<Receipt>;
}
export declare class HostSession {
    private transport;
    readonly desktop: DesktopClient;
    private constructor();
    static start(options: SessionOptions): Promise<HostSession>;
    get hello(): {
        protocol: string;
        environment: {
            epoch: string;
        };
        features: string[];
        input_mode: string;
        input_policy: string;
    };
    grant(application: Ref, id?: string): Promise<unknown>;
    declare(name: string, id?: string): Promise<unknown>;
    declareWindow(windowTitle: string, id?: string): Promise<unknown>;
    revoke(application: Ref, id?: string): Promise<unknown>;
    revokeGrant(grantId: string, id?: string): Promise<unknown>;
    grants(): Promise<GrantStatus>;
    beginTurn(turn: string): Promise<unknown>;
    endTurn(): Promise<unknown>;
    close(): Promise<void>;
}
export declare function value<T>(fact: import('./types.js').Fact<T>): T;
export declare function one(ob: Observation, name: string): import("./types.js").UIObject;

import type React from "react";

import { Icon } from "../../../shared/ui";

import "./OnboardingLayout.css";

export interface OnboardingLayoutProps {
  step: 1 | 2 | 3;
  children: React.ReactNode;
  deviceStepLabel?: string;
}

const STEPS = ["Bienvenue", "Ta Lunii", "Tes histoires"] as const;

/** The first-use journey has its own reading surface, without library navigation. */
export function OnboardingLayout({ step, children, deviceStepLabel = "Ta Lunii" }: OnboardingLayoutProps): React.JSX.Element {
  return (
    <main className="onboarding">
      <div className="onboarding__card">
        <header className="onboarding__header">
          <span className="onboarding__brand">Rustory</span>
          <ol className="onboarding__steps" aria-label="Étapes de bienvenue">
            {STEPS.map((label, index) => {
              const number = index + 1;
              const complete = number < step;
              return (
                <li
                  key={label}
                  className={`onboarding__step${complete ? " onboarding__step--complete" : ""}`}
                  aria-current={number === step ? "step" : undefined}
                >
                  <span className="onboarding__step-number" aria-hidden="true">
                    {complete ? <Icon name="check" size={14} /> : number}
                  </span>
                  <span className="onboarding__sr-only">Étape {number} : </span>
                  {index === 1 ? deviceStepLabel : label}
                </li>
              );
            })}
          </ol>
        </header>
        {children}
      </div>
    </main>
  );
}

/** A decorative device drawing, never a simulated device connection. */
export function OnboardingDeviceIllustration(): React.JSX.Element {
  return (
    <div className="onboarding__device-art" aria-hidden="true">
      <svg viewBox="0 0 340 300" focusable="false">
        <ellipse cx="170" cy="259" rx="115" ry="17" fill="#23314a" opacity=".07" />
        <circle cx="174" cy="143" r="127" fill="#E3F3EA" />
        <circle cx="282" cy="61" r="11" fill="#FFC845" />
        <path d="m49 102 5-12 5 12 12 5-12 5-5 12-5-12-12-5Z" fill="#E8618C" />
        <g transform="rotate(-7 170 160)">
          <path d="M104 73v-9c0-11 9-20 20-20h93c11 0 20 9 20 20v9" fill="none" stroke="#157957" strokeWidth="15" />
          <rect x="70" y="70" width="208" height="174" rx="32" fill="#1F7A55" />
          <rect x="77" y="74" width="194" height="158" rx="28" fill="#3DD68C" />
          <rect x="97" y="95" width="92" height="74" rx="13" fill="#FFFBF3" />
          <path d="M118 150v-30c10-5 20-5 27 0 8-5 17-5 26 0v30c-9-5-18-5-26 0-7-5-17-5-27 0Z" fill="#CFE9F8" stroke="#23314A" strokeWidth="3" strokeLinejoin="round" />
          <path d="M145 120v30" stroke="#23314A" strokeWidth="3" />
          <circle cx="229" cy="126" r="24" fill="#FFDF70" stroke="#F8CA45" strokeWidth="4" />
          <circle cx="229" cy="126" r="7" fill="#FFFBF3" />
          <circle cx="111" cy="199" r="10" fill="#FFDF70" />
          <circle cx="147" cy="199" r="10" fill="#FFDF70" />
          <circle cx="224" cy="189" r="3" fill="#186347" />
          <circle cx="237" cy="189" r="3" fill="#186347" />
          <circle cx="224" cy="202" r="3" fill="#186347" />
          <circle cx="237" cy="202" r="3" fill="#186347" />
          <circle cx="224" cy="215" r="3" fill="#186347" />
          <circle cx="237" cy="215" r="3" fill="#186347" />
        </g>
      </svg>
    </div>
  );
}
